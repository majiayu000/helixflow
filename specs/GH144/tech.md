# Tech Spec

## Linked Issue

GH-144

## Product Spec

见 `specs/GH144/product.md`。

## Codebase Context

| Area | Files | Contract |
| --- | --- | --- |
| Canonical graph | `crates/graph/src/semantics.rs` | GH145 node-embedded semantics；`migrate_v1_for_connector` 输出 typed、connector-bound report |
| Persistence | `crates/store/migrations/0007_version_migration.sql`, `version_migration_records.rs` | assessment、operation audit、`source=migration` 与 current/connector CAS |
| API | `crates/server/src/version_migration_routes.rs` | version-bound dry-run/apply、canonical report hash、replay-first |
| Derived writers | `version_semantics.rs`, layout/ops/proposal/version routes | embedded truth 校验与 sidecar 派生索引 |
| UI data | `web/src/version-migration-types.ts`, `api-version-migration.ts`, `store-version-migration.ts` | 独立 Zod DTO/API/Zustand 状态机 |
| UI entry | `web/src/components/version-migration-panel.tsx`, `run-panels.tsx` | version history 邻近入口与显式确认 |

## API Contract

### Dry-run

`POST /api/workspaces/{workspace_id}/versions/{version_id}/migration/dry-run`

server 验证 path version 是 workspace current，读取并校验 graph file，再按以下顺序：

1. 若 graph 包含 embedded semantics 或 catalog revision，使用
   `WorkflowGraph::validate_semantics`；合法即 `already_migrated`。
2. 若仅有 legacy sidecar，构造只用于验证的 embedded view；它是兼容输入，不覆盖
   embedded truth。
3. 否则验证 legacy structural graph（校验视图只移除合法 string `params.model`），再用
   current workspace connector 调用 `migrate_v1_for_connector`。
4. 验证 candidate embedded semantics，映射 typed node results，计算不含
   `applyEnabled` 的 canonical `reportHash`。
5. 追加只含 stable identity、status/code/count 的 assessment。

响应字段使用 camelCase；reason code 使用 SCREAMING_SNAKE_CASE。

### Apply

`POST /api/workspaces/{workspace_id}/versions/{version_id}/migration/apply`

请求包含 `operationId`、`reportHash`、`sourceGraphHash`、`catalogRevision`、
`workspaceConnectorId`、`migrationVersion`。

执行顺序：

1. 计算 fingerprint，并先查询 `(workspace_id, operation_id)`；同输入直接返回原 target，
   不读取 current、不重算报告、不发布 candidate。
2. replay miss 后检查 `HELIXFLOW_V1_MIGRATION_APPLY`。
3. 重算 dry-run；先检查 connector identity，再检查全部 report precondition。
4. 发布 `CandidateKind::Migration` 的 canonical embedded graph。
5. `Store::apply_version_migration` 在单 transaction 中：
   - 二次 operation lookup；
   - 插入 `source=migration` target；
   - `WHERE cur_version_id = ? AND runtime_provider_id IS ?` 推进 current；
   - 插入成功 migration audit；
   - commit。
6. commit 后 mark candidate；任何 store error 执行引用感知 cleanup。cleanup 失败不可吞掉。

## Persistence

0007 通过 table rebuild 扩展 `versions.source` CHECK，保留旧 rows、FK、index 和
`semantics_json`。新增：

- `version_migration_assessments`：append-only、secret-free 的 dry-run/conflict 聚合。
- `version_migrations`：唯一 `(workspace_id, operation_id)`，记录 fingerprint、
  source/target graph identity、migration/catalog/connector/report identity 与时间。

`semantics_json` 是从 embedded nodes 收集的派生索引。migration status 不以该列是否
为 NULL 判断；runtime 也优先读取 embedded semantics。

## Derived Version Writers

- layout：从 moved embedded graph 重建 sidecar。
- restore/undo：验证 target embedded graph并重建 sidecar。
- ops/manual proposal/agent proposal：验证 edited graph；删除 node 同步删除 semantics，
  新增或重类型 executable 没有 embedded semantics 时返回 conflict。
- legacy graph 同时没有 embedded semantics 与 sidecar 时保持 NULL，不制造假的 v2；
  sidecar-only 历史版本通过兼容视图验证并继续传播，不兼容变更 fail-closed。

## Frontend State Machine

- dry-run 携带 AbortSignal，workspace/version/connector 改变后丢弃 stale response。
- apply 不使用 AbortSignal；网络/不确定 5xx 保留原 context、report 与 operation ID，
  retry 必须复用同一 ID。
- 409、确定 4xx 与 apply-disabled 503 清除 pending 并显示 conflict/failed。
- apply 在其他 context 可见时晚到成功，缓存 target 与 `workspaceState`；返回原 context
  后 hydrate，禁止覆盖当前其他 workspace。
- panel 直接使用 server `applyEnabled`，二次点击确认，状态区域使用 `aria-live`。
- 无 current version 时不挂载 panel；dirty manual edits 时 action disabled；报告摘要显示
  server-bound connector 与 mapped/structural/unresolved 计数。
- operation ID 优先使用 `globalThis.crypto.randomUUID`，不可用时生成稳定前缀的本地
  fallback，ID 在 unknown retry 期间保持不变。

## Product-to-Test Mapping

| Invariant | Verification |
| --- | --- |
| connector-bound typed migration | graph semantics tests + server HTTP integration |
| embedded canonical detection | embedded-only dry-run returns already_migrated |
| atomic audit/replay/CAS/pending guard | store version migration tests |
| audit FK 与 migration orphan cleanup | store cascade + server reconciliation tests |
| source immutability/candidate cleanup | server/store integration tests |
| derived writer semantics | layout/ops/restore/agent proposal integration |
| UI unknown/context recovery | `web/src/version-migration.test.tsx` |

## 风险与回滚

- Security：不持久化 graph payload/raw params/secret；错误摘要稳定。
- Concurrency：current、connector 与 pending proposal 由同一 SQL CAS 封闭 assessment
  后的竞态。
- Compatibility：保留 sidecar-only runtime fallback，但新写入以 embedded graph 为真相。
- Rollback：关闭 `HELIXFLOW_V1_MIGRATION_APPLY`；不删除已提交 migration version。
