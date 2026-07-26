# Tech Spec

## Linked Issue

GH-144

## Product Spec

见 `specs/GH144/product.md`。

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Graph migrator | `crates/graph/src/graph_v2.rs`、`graph_v2_tests.rs` | `migrate_v1_to_v2` 返回 `Option<WorkflowGraphV2> + MigrationReport`；保持 topology，只有全部 executable node 可解析时才返回 migrated graph | 复用确定性核心；补稳定 reason code，禁止 server 解析自由文本错误 |
| Catalog | `crates/server/src/catalog_routes.rs`、`crates/registry/src/catalog.rs` | `shared_catalog()` 提供进程内 canonical `CatalogSnapshot`；migrator 根据 catalog 数据而非 connector runtime health 决策 | dry-run/apply 必须绑定同一 `catalogRevision`，catalog 变化使报告过期 |
| Version store | `crates/store/src/lib.rs`、`version_records.rs`、migration `0006_version_semantics.sql` | version graph 存文件，record 保存 `graph_path`/`graph_hash`；v2 语义层保存于 `semantics_json`；current version 用事务 CAS 推进 | apply 创建新 version，不能原地改写旧 v1 record |
| File commit | `crates/server/src/version_file_consistency.rs` | `VersionFileCandidate` 先安全发布 immutable graph 文件，store 失败后检查引用并清理；启动时 reconciliation 处理一致性 | migration apply 必须复用同一 publish→DB commit→mark/cleanup 协议 |
| Version routes | `crates/server/src/version_routes.rs`、`main.rs` | undo/restore/apply 均围绕 workspace current version；全 Router 由 `app_with_auth` 统一保护 | 新 endpoint 放在同一 Router，复用 current-version conflict 语义 |
| API errors | `crates/server/src/api_error.rs` | 支持 `bad_request`、`conflict_with_details`、`not_found_with_details` 等安全响应 | migration 的 expected failures 返回稳定 `code`/details，内部错误不泄漏路径或 provider payload |
| Web API/store | `web/src/api.ts`、`store.ts`、`store-types.ts`、`types.ts` | Zod 校验 API；Zustand action 在 workspace generation 变化时丢弃 stale response；核心文件已接近 U-16 上限 | 新 migration API/types/actions 独立文件，避免继续扩大 741/797/685 行文件 |
| Version UI | `web/src/components/run-panels.tsx`、`run-panels.test.tsx` | history panel 展示 current/restore/undo，但没有 schema/migration 状态 | 在 history 邻近增加独立 migration panel，复用当前 workspace/version truth |

## 设计方案

### 1. 边界与 endpoint

新增两个只处理 path 中 workspace 当前 version 的 endpoint：

```text
POST /api/workspaces/{workspace_id}/versions/{version_id}/migration/dry-run
POST /api/workspaces/{workspace_id}/versions/{version_id}/migration/apply
```

两条 route 都注册在 `app(state)`，因此自动继承 `app_with_auth`。不新增独立 auth
或 ACL 旁路。`version_id` 必须属于 `workspace_id` 且等于 `cur_version_id`；historical
version 只能先通过既有 restore 行为成为 current version 后再迁移。

dry-run 无 request body，不接受客户端 catalog、graph 或 model mapping。apply request：

```json
{
  "operationId": "client-stable-id",
  "reportHash": "sha256:...",
  "sourceGraphHash": "sha256:...",
  "catalogRevision": "catalog-v2",
  "migrationVersion": "1"
}
```

API 边界使用 camelCase；Rust 字段、数据库列和稳定内部 ID 使用 snake_case。
客户端只提交 precondition，server 必须重新读取 source graph 并重新运行 migrator；
绝不接收客户端生成的 migrated graph 或 semantics。

### 2. 外部报告 DTO

不直接暴露 `MigrationReport` 的 Rust enum 序列化细节。新增显式 DTO：

```rust
struct VersionMigrationReport {
    status: VersionMigrationStatus,
    migration_version: String,
    workspace_id: String,
    source_version_id: String,
    source_graph_hash: String,
    source_schema_version: u32,
    catalog_revision: String,
    report_hash: String,
    nodes: Vec<NodeMigrationResult>,
}

enum VersionMigrationStatus {
    Migratable,
    NeedsResolution,
    AlreadyMigrated,
    Failed,
}

struct NodeMigrationResult {
    node_id: String,
    action: NodeMigrationAction,
    code: Option<MigrationReasonCode>,
    message: Option<String>,
    candidates: Vec<String>,
}
```

`nodes` 按 source graph 的稳定 node ID 顺序输出；`candidates` 排序并去重。
`reportHash` 是排除自身字段后，对完整安全 DTO canonical JSON 计算的 SHA-256。

`MigrationAction::NeedsResolution` 从自由文本 `reason` 改为 typed
`MigrationReasonCode + safe message`。首批稳定 code 至少覆盖：

```text
UNKNOWN_NODE_TYPE
CAPABILITY_NOT_FOUND
MODEL_NOT_FOUND
MODEL_AMBIGUOUS
MODEL_CAPABILITY_MISMATCH
BINDING_NOT_FOUND
BINDING_AMBIGUOUS
DEFAULT_BINDING_MISSING
SOURCE_GRAPH_MISSING
SOURCE_GRAPH_HASH_MISMATCH
SOURCE_GRAPH_INVALID
SEMANTICS_INVALID
```

server 根据 typed value 映射 DTO，不按 message 字符串匹配。message 只作人类说明，
不得包含绝对路径、credential、endpoint 或 provider 原始响应。

### 3. dry-run pipeline

`version_migration_routes.rs::dry_run_version_migration` 按以下顺序执行：

1. 读取 workspace、确认 current version 与 path `version_id` 一致。
2. 读取 version record；若 `semantics_json` 存在，反序列化并用当前 graph/catalog
   validator 验证。合法则返回 `already_migrated`；无效则返回 `failed`，不能当作 v1。
3. 通过 `read_version_graph` 验证 safe path、canonical hash 与 JSON。
4. 获取 `shared_catalog()`；migrator 继续只使用 catalog declarations，不把临时
   connector health 当作 migration mapping。
5. 调用 `migrate_v1_to_v2`，将 typed report 转为外部 DTO 并计算 `reportHash`。
6. 返回 DTO；不写 database、graph file、event、message 或临时 report。

任一 node unresolved 时顶层为 `needs_resolution` 且不返回可提交的 graph。
graph/file 可预期损坏以 `failed` 报告返回；数据库不可用等基础设施错误使用现有
`ApiError` 5xx 路径。

### 4. apply、并发与幂等

apply 先按 dry-run pipeline 重新计算 server report，然后逐项比较
`reportHash`、`sourceGraphHash`、`catalogRevision`、`migrationVersion`。任一不一致
返回 `REPORT_STALE` conflict。report 非 `migratable` 时不进入写路径。

成功写入流程：

1. 将 `WorkflowGraphV2.base` 建成 `CandidateKind::Migration`；
   `WorkflowGraphV2.semantics` 按现有 T6 格式序列化到 `semantics_json`。
2. publish candidate，获得新的 `graph_path` 与 `graph_hash`。
3. 调用单一 store transaction `apply_version_migration`：
   - `BEGIN IMMEDIATE`；
   - 查询 `(workspace_id, operation_id)`；
   - 已存在且 fingerprint 相同则返回已提交 record；
   - 已存在但 fingerprint 不同则返回 `OperationIdConflict`；
   - CAS 校验 `workspaces.cur_version_id == source_version_id`；
   - 插入 source=`migration` 的新 version；
   - CAS 推进 current pointer；
   - 插入成功 migration audit record；
   - commit。
4. 新提交调用 `candidate.mark_committed()`；store 失败调用
   `cleanup_after_store_error()`，不得吞掉 cleanup failure。
5. 返回 target version identity 和 fresh `workspace_state_value`。

operation fingerprint 覆盖 workspace/source version/source graph hash/report hash/catalog
revision/migration version，不覆盖 server 生成的 target ID。同 operation 重试从 audit
record 返回原 target，不创建 candidate；并发不同 operation 由 current-version CAS
保证最多一个提交成功。

### 5. 持久化

在 `crates/store/migrations/` 的下一条 migration 新增 `version_migrations`：

```text
id
workspace_id
operation_id
operation_fingerprint
source_version_id
source_graph_hash
target_version_id
target_graph_hash
migration_version
catalog_revision
report_hash
report_json
created_at
UNIQUE(workspace_id, operation_id)
```

`report_json` 只保存已经过 secret-free DTO 转换的成功 apply report。dry-run 不落库。
新增 `VersionSource::Migration`，字符串值固定为 `migration`。audit record 与 target
version 在同一 transaction 插入，禁止出现“已迁移 audit、但 target version 不存在”
或反向状态。

不更新 source version 的 `semantics_json`、graph file、label 或 hash。原 v1 version
继续作为 immutable history；新 v2 version 的 `parent_id` 指向 source version。

### 6. Web 结构与状态机

为避免现有大文件继续膨胀，新增：

- `web/src/version-migration-types.ts`：Zod schemas 与 DTO types。
- `web/src/api-version-migration.ts`：dry-run/apply requests。
- `web/src/store-version-migration.ts`：独立 Zustand action/state factory。
- `web/src/components/version-migration-panel.tsx` 及测试。

`store-types.ts` 仅组合新的 `VersionMigrationSlice`；`store.ts` 仅接入 factory，不放置
迁移业务逻辑。panel 放在 version history 邻近，只对 current version 展示入口。

状态机：

```text
idle
  → loading_report
  → migratable | needs_resolution | already_migrated | report_failed
migratable
  → explicit_confirm
  → applying
  → succeeded | conflict | apply_failed
```

workspace generation 或 current version 变化时清空 report/confirm 状态并 abort 未完成
request。apply 网络结果未知时保留 `operationId`，通过相同 operation 重试并随后 hydrate
workspace；不得本地猜测 target version。node 结果用语义化列表呈现，状态和按钮具备
文本标签、键盘可达性与 `aria-live` 更新。

### 7. Rollout gate

新增 server config `HELIXFLOW_V1_MIGRATION_APPLY`，默认 `0`：

- dry-run endpoint 与报告 UI 随功能发布。
- flag 为 `0` 时 apply route 返回带 `MIGRATION_APPLY_DISABLED` 的 503，UI 只展示报告。
- 观察 dry-run reason code 分布和 secret-free contract 后，由维护者显式设为 `1`。
- #145 接手 migration coverage gate，并决定何时移除本临时 flag；#146 不复用此 flag
  作为 legacy proposal 回滚开关。

flag 只控制 apply，不改变 graph read、run 或 existing v2 write 行为。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 | route path validation、workspace/current lookup | workspace/version mismatch、historical/nonexistent version route tests |
| P2 | dry-run handler | DB/file snapshot before/after；assert 零 row/file/current 变化 |
| P3 | graph migrator、DTO canonical encoding | repeated dry-run byte equality test |
| P4 | DTO status/node mapping | exhaustive status/action schema tests，node 一一对应 |
| P5 | typed migration reasons、resolver | missing/ambiguous/model mismatch/default binding fixtures |
| P6 | migrated base graph | node/edge/topology canonical comparison，legacy model field only allowed delta |
| P7 | UI node report、apply guard | unresolved node rendering + apply disabled integration tests |
| P8 | apply request/precondition validation | tampered/missing report precondition rejected |
| P9 | current CAS、report recomputation | graph/catalog/migration/current stale conflict tests |
| P10 | candidate + store transaction | injected publish/store/current-update failures，无 orphan/half commit |
| P11 | append-only version creation | source version row/file/hash unchanged，target parent 正确 |
| P12 | `version_migrations` unique key/fingerprint | same operation replay、different payload conflict、concurrent operation tests |
| P13 | v2 detection/validation | valid v2 returns already；invalid semantics returns failed；零新 version |
| P14 | migration slice/panel | loading/success/conflict/failure + workspace switch stale-response tests |
| P15 | `app_with_auth` route placement | authenticated/unauthenticated route contract tests |
| P16 | typed safe errors、DTO serialization | secret/endpoint/header/path sentinel absence tests |
| P17 | migration audit store/query | source/target/hash/revision/report round-trip and coverage aggregation tests |

## 数据流

```text
UI current workspace/version
  → POST dry-run
    → verify workspace + current version
    → verify graph file/hash
    → shared catalog + migrate_v1_to_v2
    → safe deterministic report + reportHash
  → user explicit confirm
  → POST apply(operationId + preconditions)
    → recompute and compare report
    → stage/publish migrated base graph candidate
    → one SQLite transaction:
         new migration version
         + current-version CAS
         + migration audit
    → mark candidate committed
    → fresh workspace state
  → UI hydrate server truth
```

无 provider 调用、Agent 调用或外部网络调用。migration mapping 只依赖 source graph、
`NodeRegistry::builtin()` 与 canonical catalog snapshot。

## 备选方案

- **原地更新旧 version 的 `semantics_json`**：否。破坏 immutable history，失败时难以
  区分原始 v1 与部分 backfill，也削弱回滚证据。
- **客户端提交 migrated graph**：否。客户端可篡改 topology/binding，无法保证 report
  与 apply 同源。
- **dry-run 后缓存完整 graph 并直接 apply**：否。cache 会引入过期和恢复复杂度；
  apply 重新计算并比较 identity 更简单且 fail-closed。
- **一次请求迁移整个 workspace history**：否。本期以 current version 为原子边界，
  避免长事务和部分成功语义；历史 version 继续只读保留。
- **复用自由文本 `reason` 作为稳定 code**：否。文本变化会破坏 API/UI contract。
- **把新逻辑继续加入 `web/src/store.ts`/`api.ts`/`types.ts`**：否。这些文件已接近
  U-16 上限，新能力使用独立模块。

## 风险

- Security: migration report 可能携带 legacy params 中的敏感内容。DTO 只输出 node ID、
  typed code、canonical model/binding candidates 和安全摘要，不回显 params；route 必须
  位于现有 auth middleware 内。
- Compatibility: v1 history 仍存在且可 restore。#145 删除 legacy read path 前必须定义
  historical v1 的隔离/再迁移策略；本 issue 不声称完成全库 backfill。
- Performance: dry-run/apply 各读取并遍历一次 current graph；本期不做 workspace batch，
  避免长事务。对异常大 graph 依赖既有请求限制并记录耗时。
- Data integrity: file publish 与 SQLite 不能组成单一原子事务。必须复用 candidate
  ownership、store-reference-aware cleanup 和 startup reconciliation，不能自行
  `write` 后忽略失败。
- Concurrency: dry-run 后的任何 edit/proposal/restore 都使报告 stale。server 以 current
  version CAS 为最终真相，UI disabled 状态不是并发保护。
- Maintenance: typed reason code 是外部 contract；新增 code 可扩展，已有 code 不得静默
  改义或复用。

## 测试计划

- [ ] Unit tests: graph typed reason code、report canonicalization/hash、operation
      fingerprint、DTO secret-free serialization、web Zod/state machine。
- [ ] Store tests: migration audit round-trip、same-operation replay、operation conflict、
      current CAS、transaction fault injection、source immutability。
- [ ] Server integration tests: dry-run 零写入、apply success、unresolved、already migrated、
      stale graph/catalog/current、missing/corrupt graph、auth、flag disabled、candidate cleanup。
- [ ] Concurrency tests: same operation 与不同 operation 双请求，验证最多一个 target
      version、无 orphan file、replay 返回同一 target。
- [ ] Web tests: report node rendering、explicit confirm、apply disabled、conflict refresh、
      network unknown retry、workspace/version switch 丢弃 stale response、accessibility labels。
- [ ] Deterministic commands:
      `cargo test -p helixflow-graph -p helixflow-store -p helixflow-server`；
      `cargo check --workspace`；`cargo test --workspace`；
      `cd web && npx tsc --noEmit && npm test`。
- [ ] Spec checks:
      `python3 checks/check_workflow.py --repo . --spec-dir specs/GH144`；
      `python3 checks/check_workflow.py --repo . --all-specs`。
- [ ] Manual verification: 对含 policy、pinned、ambiguous 和 corrupt graph 的 workspace
      逐项 dry-run；确认 UI 报告、apply gate、version history 和 retry 与 server truth
      一致。

## 回滚方案

1. 将 `HELIXFLOW_V1_MIGRATION_APPLY=0`，立即禁止新 apply，保留 dry-run 诊断。
2. 前端隐藏/禁用 apply 操作，但继续展示报告和已完成 version history。
3. 不删除、降级或覆盖已创建的 v2 version；用户可通过既有 restore 创建指向原 v1
   version 内容的新 current version。
4. 若新 route 本身有问题，可回滚 server/web 代码；新增 migration audit table 保留为
   未使用的向后兼容数据，不执行 down migration 删除记录。
5. 若 commit 后发现 candidate/store 不一致，使用既有 startup reconciliation 与
   store-reference-aware cleanup；不得手工批量删除 graph 文件。
