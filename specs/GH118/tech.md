# Tech Spec

## Linked Issue

GH-118（https://github.com/majiayu000/helixflow/issues/118）

## Product Spec

见 `specs/GH118/product.md`。实现基线为包含 PR #121/#122 的 `origin/main` `0471c213`。

## Planned Changes Manifest

```specrail-planned-changes
{
  "issue": 118,
  "complete": true,
  "paths": [
    "crates/server/src/app_state.rs",
    "crates/server/src/artifact_retry_tests.rs",
    "crates/server/src/artifact_routes.rs",
    "crates/server/src/graph_files.rs",
    "crates/server/src/layout_routes.rs",
    "crates/server/src/main.rs",
    "crates/server/src/ops_routes.rs",
    "crates/server/src/ops_routes_tests.rs",
    "crates/server/src/proposal_routes.rs",
    "crates/server/src/run_routes.rs",
    "crates/server/src/run_routes_tests.rs",
    "crates/server/src/version_file_consistency.rs",
    "crates/server/src/version_file_consistency_tests.rs",
    "crates/server/src/version_file_reconciliation.rs",
    "crates/server/src/version_file_reconciliation_tests.rs",
    "crates/server/src/version_routes.rs",
    "crates/server/src/workbench_message.rs",
    "crates/server/src/workbench_message/gh102_tests.rs",
    "crates/server/src/workbench_message/gh60_tests.rs",
    "crates/server/src/workbench_message_graph.rs",
    "crates/server/src/workbench_message_graph_tests.rs",
    "crates/server/src/workbench_message_proposals.rs",
    "crates/server/src/workbench_message_tests.rs",
    "crates/server/src/workspace_canvas.rs",
    "crates/server/src/workspace_routes.rs",
    "crates/server/src/workspace_state.rs",
    "crates/server/src/workspace_state_tests.rs",
    "crates/store/src/lib.rs",
    "crates/store/src/proposal_records.rs",
    "crates/store/src/proposal_records_apply_tests.rs",
    "crates/store/src/proposal_records_auto_apply_tests.rs",
    "crates/store/src/version_file_reference_records.rs",
    "crates/store/src/version_file_reference_records_tests.rs",
    "crates/store/src/version_records.rs",
    "crates/store/src/version_records_tests.rs",
    "crates/store/src/workspace_initialization_records.rs",
    "crates/store/src/workspace_initialization_records_tests.rs",
    "crates/store/src/workspace_records.rs"
  ],
  "spec_refs": [
    "B-001", "B-002", "B-003", "B-004", "B-005",
    "B-006", "B-007", "B-008", "B-009", "B-010",
    "B-011", "B-012", "B-013", "B-014", "B-015"
  ]
}
```

Manifest 是封闭集合；若实现发现还需修改其他 production/test 路径，必须先修订 spec，不得以“fixture 顺手调整”为由越界。明确排除 PR #122/GH-120 所有文件：`crates/server/src/canvas_collaboration.rs`、`crates/server/src/canvas_collaboration_tests.rs`、`crates/store/src/canvas_comment_records.rs`、`crates/store/migrations/0005_canvas_comment_records.sql`、Web comments/presence/WebSocket 文件及 `comments.json`。

## 类型与状态所有权

本 issue 是边界接线补全，不创建新 crate。唯一提交真相是 SQLite：`versions` 的 `VersionRecord` 与 `workspaces.cur_version_id` 决定当前 graph；`proposals`/`messages` 决定 proposal/message 状态。filesystem 只保存由 DB 记录引用并以 `graph_hash` 验证的 payload，是 adapter，不得从“文件存在”推断提交成功，也不得把候选目录当第二套状态机。

| 边界 | 唯一 owner | 允许依赖 | 禁止依赖 | 契约测试 |
| --- | --- | --- | --- | --- |
| current/version commit | Store SQLite transaction | server 先发布 immutable candidate，再调用 Store CAS | filesystem 路径决定 current；DB commit 后再覆盖文件 | `version_records_tests`、route concurrency tests |
| initial workspace commit | Store `create_workspace_with_initial_version` | 预分配 identity、已发布 candidate | 先插 workspace 再补 version/current | `workspace_initialization_records_tests` |
| proposal apply 状态 | Store proposal transaction | 已发布 applied/ops/preview candidates | route 在 transaction 后另写 applied message | `proposal_records_apply_tests`、`proposal_records_auto_apply_tests` |
| graph payload | server filesystem adapter | exclusive publish、DB reference lookup、verified read | 普通覆盖 rename、未校验反序列化、未知时删除 | `version_file_consistency_tests` |
| request graph | current `VersionRecord` verified bytes | 客户端 graph 只做等值验证/selection context | agent/run/sweep 直接消费客户端 graph | `workbench_message_graph_tests` |
| startup health | `AppState` retained `ReconciliationReport` | Store enumeration + filesystem scan | warning 后继续、日志泄漏绝对路径/内容 | startup reconciliation tests |

## 当前代码事实

| Area | Latest-main callsite | 当前行为 | 必须修正 |
| --- | --- | --- | --- |
| 通用 graph I/O | `graph_files.rs:18-113` | `read_graph_file` 忽略 hash；`write_json_file` 覆盖 final | 只保留安全 JSON primitive；version read/publish 迁入 consistency 模块 |
| Initial workspace | `workspace_routes.rs:44-84`、`store/lib.rs:170-184,222-338` | workspace row、文件、version 分三步 | 预分配 workspace/version ID；文件先行；一个 Store transaction 写三项 |
| Layout | `layout_routes.rs:72-114,155-160` | `layout-{base}.json` 可被并发覆盖；用非 pending-guard transaction | unique candidate + `create_version_after_without_pending_proposal` |
| Manual ops | `ops_routes.rs:84-172,400-479` | 自有 temp/rename/CAS；rename 可覆盖、cleanup 吞错 | 删除本地 helper，接统一 candidate/cleanup；保留严格 idempotency |
| Manual proposal | `proposal_routes.rs:23-88`、`proposal_records.rs:223-329` | version/current/proposal 提交后 route 单独 `create_message` | 扩展同一 Store transaction 创建 applied message并返回 result |
| Agent auto-apply | `workbench_message_proposals.rs:15-99`、`proposal_records.rs:331-483` | 三文件覆盖写；DB 已把 version/current/message/proposal 放在一个 transaction | 文件改 candidate set；保留并补强既有 DB transaction/fault tests |
| Stored reads | `workspace_state.rs:40-49`、`workspace_canvas.rs:29-39`、`run_routes.rs:56-65`、`version_routes.rs:14-25,111-136` 及 writer base reads | 只传 `graph_path` | 全部传 `&VersionRecord` 到 verified helper |
| Workbench ingress | `workbench_message.rs:48-101`、`workbench_message_canvas.rs:18-55`、`sweep_support.rs:50,150` | classification、agent 和 run/sweep 使用客户端 `input.graph` | ingress 取 current VersionRecord、verified read、比对客户端后只传 server graph |
| Startup | `app_state.rs:35-45`、`main.rs:64-71` | migration 后直接 stale-run cleanup/provider status | 先 reconciliation；report 留在 AppState 且监听前写结构化日志 |
| Oversized tests | `workspace_state.rs` 852 行；`run_routes.rs` 785 行；`workbench_message.rs` 746 行 | inline tests 推高生产文件 | 在逻辑编辑前先移到 manifest 中三个 `*_tests.rs` 文件 |

## 设计

### 1. Store 文件拆分与明确 transaction API

先做纯机械拆分并保持测试绿：

- `version_records.rs` 接管 `create_version`、`create_version_after`、`create_version_after_without_pending_proposal`、内部 `insert_version` 与 `version`；原 `lib.rs` 只保留模块注册、共用类型/error/connect。
- `workspace_records.rs` 接管现有 workspace CRUD/list/message/list-version 查询，不改变 `create_workspace` 的现有测试/内部兼容语义；production `workspace_routes` 不再调用两阶段 `create_workspace`。
- `proposal_records_apply_tests.rs` 接管并扩充人工 apply transaction tests；auto-apply 继续由 `proposal_records_auto_apply_tests.rs` 覆盖。

新增无 DB 副作用的 `Store::reserve_workspace_identity() -> ReservedWorkspaceIdentity { workspace_id, initial_version_id }`。ID 仍由 Store 的 opaque UUID 规则生成，调用只分配 identity，不插行。`workspace_routes` 用预分配 workspace ID 和独立 candidate UUID 构造 workspace-scoped path并先发布文件，然后调用：

```text
Store::create_workspace_with_initial_version(
  CreateWorkspaceWithInitialVersion {
    identity, name, version_label, source, graph_path, graph_hash
  }
) -> StoreResult<InitializedWorkspace { workspace, version }>
```

`workspace_initialization_records.rs` 在单个 SQLite transaction 中使用预分配 ID 插入 workspace、插入 initial version、设置 `cur_version_id`，检查每条语句 `rows_affected == 1`，commit 后按预分配 ID读回。ID/path collision 或任一 SQL/commit error 都不允许部分 DB 行；server 用预分配 version/path做精确引用复查，commit 结果不明时也不会查询全局未知路径。

人工 proposal 的 `ApplyProposalVersionRecord` 增加 `message_text`；`create_version_after_applying_proposal` 返回 `ApplyProposalVersionResult { version, message }`，并在现有 version insert/current CAS/proposal applied update transaction 内插入当前 route 已创建的 `proposal_applied` message（`ref_id=proposal_id`、attachment 含 version ID）。`proposal_routes.rs` 删除 transaction 后的独立 `create_message`。auto-apply API 不改 shape，只补 fault assertions。

`version_file_reference_records.rs` 提供两个精确、只读 API：

```text
Store::version_file_references(relative_path) -> StoreResult<Vec<VersionRecord>>
Store::proposal_file_references(relative_path) -> StoreResult<Vec<ProposalFileReference>>
```

查询分别匹配 `versions.graph_path` 与 `proposals.ops_path/preview_graph_path`；返回 Vec 以便把意外重复引用显式当 integrity error，而不是任取一条。candidate cleanup 要求两类均明确无引用。查询失败即 `unknown`，保留文件并返回 cleanup-deferred 错误。startup 仍使用 `workspaces` + `versions_for_workspace` + `workspace_proposals` 全量枚举。

### 2. Exclusive candidate publication

`version_file_consistency.rs` 定义私有 `VersionFileCandidate`/`VersionFileCandidateSet`：持有 workspace、kind、relative temp/final、canonical bytes、`sha256:` hash 与 ownership nonce。无 idempotency key 的 final 使用 `<kind>-<uuid_v7>.json`；temp 与 final 同目录并带独立 nonce。

manual ops 的 keyed 逻辑身份是 `sha256("helixflow:ops-idempotency:v1" || len(workspace_id) || workspace_id || len(base_version_id) || base_version_id || len(idempotency_key) || idempotency_key)`，长度采用固定大端整数，final 为 `ops-key-<64 lowercase hex>.json`。raw key 不进入路径、错误或日志。同 workspace/base/key 必然得到同一路径；route 必须 verified read base、应用 ops并计算候选 hash，再允许 replay：`version_file_references(path)` 必须恰好返回一条 workspace、parent(base)、path、graph_hash 都精确匹配的记录。路径存在但 hash/parent/workspace 不同、出现多条 DB 引用，或文件存在但 DB 尚无引用，均 conflict 且不覆盖/不删除。不同 key 即使生成相同内容也产生不同路径，禁止扫描 hash/content 复用别的 key。idempotency helper 与这些断言放在 `ops_routes.rs`/`ops_routes_tests.rs`，不新增 schema。

固定发布步骤：安全 relative path → `create_new` temp → `write_all` → file `sync_all` → 同一文件系统 `hard_link(temp, final)` 作为 atomic exclusive no-replace publish → parent directory sync → 删除 temp。普通 `rename` 或任何能替换 existing final 的调用禁止；hard-link 不可用/目标已存在时 fail closed，不降级覆盖。private `CandidateIo` seam 只暴露上述操作给生产实现与 deterministic fake，不成为 public `Any` API。

所有 DB 调用由 candidate coordinator 包裹。Store 成功后 candidate 标记 committed；Store 返回任何 error（包括 commit outcome 不明）后，按 exact relative path调用两类 reference API：

- DB 明确引用：保留 candidate，返回原 transaction outcome-unknown/error；
- 两类都明确未引用：仅删除本 ownership nonce 对应的 temp/final；
- 任一查询/删除失败：保留，返回结构化 cleanup-deferred error。

候选 API 不接受任意 caller path；失败请求不能删 winner 或 legacy 文件。

### 3. Writer callsite wiring

| Callsite | Candidate set | Store commit API | Route-specific assertion |
| --- | --- | --- | --- |
| `workspace_routes::create_workspace` | initial graph 1 个；预分配 workspace/version identity | `create_workspace_with_initial_version` | workspace/version/current 全有或全无 |
| `layout_routes::save_workspace_layout` | unique layout graph 1 个 | `create_version_after_without_pending_proposal` | current CAS 与 pending-proposal guard 都在 transaction 内 |
| `ops_routes::apply_workspace_ops` | unkeyed UUID 或 keyed opaque-digest ops graph 1 个 | `create_version_after_without_pending_proposal` | exact path + workspace + parent + canonical hash匹配才 replay；同 key不同 payload、不同 key同内容均不得误 replay |
| `proposal_routes::apply_workspace_proposal` | unique applied graph 1 个 | expanded `create_version_after_applying_proposal` | version/current/proposal/applied message 同 transaction |
| `workbench_message_proposals::persist_and_apply_agent_proposal` | ops、preview、applied 三个候选 | existing `auto_apply_proposal_version` | 三文件逐一 reference-check；DB 四类记录仍全事务 |

布局不再调用无 pending guard 的 `create_version_after`。人工/agent proposal base graph 都先 verified read。candidate set 只有在所有文件 publish 成功后才进入 Store；中途失败反向清理本 set，任一清理失败均显式返回。

### 4. Verified read 与 workbench ingress

`read_version_graph(data_dir, &VersionRecord)` 固定执行：安全 relative path解析 → 读 raw bytes → 验证 `graph_hash` 严格符合 `sha256:<64 lowercase hex>` → SHA-256 精确比较 → JSON decode。错误只包含 category、version/workspace ID 与相对路径类别，不含 data-dir 绝对路径或原始 JSON。

替换这些 version consumer：

- `workspace_state` current graph；`workspace_canvas` current graph；`run_routes::queue_workspace_run`；`version_routes` export/restore；
- layout/ops/manual proposal/agent auto-apply 的 current/base graph；
- `create_restore_version` 在 CAS 前 verified read target；
- `workbench_message_graph::verified_message_graph`。

T5 可在 `artifact_routes.rs` 的 inline tests 与 `artifact_retry_tests.rs` 中仅更新因 strict verified-read 暴露的 version fixture，使 `graph_hash` 来自 fixture 实际 bytes；不得改变 artifact production route、review/rerun 状态机或断言语义。

proposal ops/preview 不是 `VersionRecord`，继续走安全 `read_json_file`；不得伪造 hash。startup 对它们验证路径、存在性与预期 JSON 类型。

`workbench_message_graph.rs` 提供：

```text
verified_message_graph(state, workspace_id, base_version_id, client_graph)
  -> Result<VerifiedMessageGraph { version, graph }, ApiError>
```

它要求 workspace current 等于 `base_version_id`、读取该 `VersionRecord`、verified read，并要求客户端 graph 与服务器 graph 结构等值；不一致返回 conflict。`post_workspace_message` 在 classification 和首条 user message 持久化之前调用它，随后 classification、canvas selection、`AgentSessionRequest.graph`、RunRequest/sweep 全部只使用返回的 server graph。客户端字段保留以维持 API wire shape，但仅承担 stale/tamper validation 与 selection context，不是执行输入。`workbench_message.rs` 只保留一次 helper 调用与变量替换，新判断集中在新模块。

### 5. Startup reconciliation 与可观测结果

`AppState::open` 顺序固定：data-dir/Store migration → `reconcile_version_files` → stale-run cleanup → provider status → construct state。reconciliation：

1. 枚举所有 workspace versions，逐个 verified read；枚举 proposal ops/preview，验证 safe path、存在性和 JSON 类型。
2. 扫描只匹配本机制 UUID candidate/temp 命名的路径；对每条调用 Store reference APIs，明确无引用才删除。
3. unknown legacy 文件不删；引用缺失/非法/hash mismatch、引用查询失败、recognized orphan 删除失败均 fail closed。

`ReconciliationReport` 至少包含 `verified_versions`、`verified_proposal_files`、`removed_orphans`、`retained_unknown`、`corrupt_references` 与各 path category count。成功报告以 `Arc<ReconciliationReport>` 留在 `AppState`，`main` 在 bind/listen 前输出单行 JSON event `version_file_reconciliation`。`AppStateError::VersionFileConsistency` 持有结构化 error code/category/record IDs；Display/log 只输出脱敏字段，绝不输出绝对 data-dir、原文件内容、SQL 或 secret-like text。test constructors 注入 empty/test report，不跳过 production open gate。

### 6. 兼容路径的收敛/删除条件

| Compatibility path | 立即收敛 | 保留条件 | 删除/退出条件 |
| --- | --- | --- | --- |
| DB-referenced legacy version graph path | startup + every read 都校验 safe path/可信 hash/JSON | VersionRecord 仍引用且验证通过 | GH-118 不删除 referenced legacy；自然创建的新 version 使用 UUID candidate。未来只有在 version retention 明确删除 DB 引用后才可由独立 GC 删除 |
| unknown legacy unreferenced file | startup 分类为 `retained_unknown` 并报告 | 命名不满足 GH-118 ownership proof | GH-118 永不自动删；需未来显式迁移/运维决策 |
| existing proposal ops/preview | startup/read 校验 safe path、存在性、预期 JSON | proposal row 仍引用 | GH-118 不删 referenced payload；新 proposal 改用 exclusive candidate。未来 proposal retention 删除 DB 引用后才可由独立 GC 删除 |
| recognized GH-118 temp/final candidate | startup 按命名 + DB reference 查询分类 | 有引用或查询不明则保留/fail closed | 两类 Store 查询均明确无引用时由 startup 删除 |

Artifact route/retry 测试中的 version graph 只属于测试兼容 fixture；strict verified-read 生效后必须使用真实 bytes/hash，不能以假 hash 绕过，也不能因此改变 artifact production 行为。

本 issue 不提供“关闭 hash 校验”的兼容开关，也不迁移存量路径。

### 7. 文件大小与测试拆分前置

在对 `workspace_state.rs` 作任何逻辑修改前，把其 `#[cfg(test)] mod tests` 原样移动到 `workspace_state_tests.rs` 并由 `main.rs` 注册；拆分后先运行原测试。相同方式把 `run_routes.rs` inline tests 移到 `run_routes_tests.rs`、`workbench_message.rs` inline tests 移到 `workbench_message_tests.rs`；保留 `workbench_message/gh60_tests.rs`、`gh102_tests.rs` 的模块可见性并更新真实 hash fixture。production route 只做 verified helper 调用，新 candidate/reconciliation/message graph 判断分别放进新模块，避免再次逼近 800 行。

## 精确故障与并发映射

| Fault/race | 注入位置 | 必须断言 | Test path |
| --- | --- | --- | --- |
| temp create/write/sync | private fake `CandidateIo` 对应 method | 无 final、无 DB row；cleanup error 可见 | `version_file_consistency_tests.rs` |
| final publish collision | fake/real pre-created final + `hard_link` | no-replace conflict；existing bytes不变 | `version_file_consistency_tests.rs` |
| parent sync/remove failure | fake `CandidateIo` | 不返回成功；只触碰 owned path | `version_file_consistency_tests.rs` |
| workspace insert/version insert/current update | SQLite TEMP trigger 分别 `RAISE(ABORT, ...)` | workspace/version/current 全无 | `workspace_initialization_records_tests.rs` |
| initial commit-then-error ambiguity | candidate coordinator future 先调用真实 Store成功、再返回 synthetic error | exact version reference 查到；final 保留；无重复 workspace | `version_file_consistency_tests.rs` |
| version insert/current CAS/pending guard | trigger + stale base + pending fixture | transaction rollback；winner current/hash不变 | `version_records_tests.rs`、layout/ops tests |
| manual proposal version/current/proposal/message each statement | TEMP trigger 按表/条件失败 | 四类 DB 状态全部回滚；candidate cleanup按引用结果 | `proposal_records_apply_tests.rs`、proposal route tests |
| auto proposal version/current/message/proposal each statement | 扩充现有 trigger/barrier fixtures | 四类记录 all-or-none；三候选无误删 | `proposal_records_auto_apply_tests.rs`、workbench tests |
| same-base layout/ops/manual/auto proposal race | `tokio::sync::Barrier` 同时进入 Store CAS | one winner；loser path不引用且被清；winner bytes/hash不变 | layout/proposal/workbench tests、`ops_routes_tests.rs` |
| reference lookup failure | SQLite trigger/closed pool after candidate publish | candidate 保留；cleanup-deferred error | `version_file_reference_records_tests.rs`、consistency tests |
| verified read missing/bad hash/bad JSON | mutate referenced file/record fixture | consumer fail closed，无 user message/run/version/restore side effect | route/read test files in manifest |
| workbench client/server mismatch | same base ID + altered client graph；stale base ID | conflict before user message/agent/run | `workbench_message_graph_tests.rs` |
| startup referenced corruption/orphan/unknown | isolated AppState data-dir fixtures | corrupt blocks open；recognized orphan removed；unknown retained；success report retained/loggable | `version_file_reconciliation_tests.rs`、app state tests |

## Product-to-Test Mapping

| Invariant | Implementation | Fresh verification |
| --- | --- | --- |
| B-001, B-003 | candidate bytes/hash + exclusive publication | `cargo test -p helixflow-server version_file_consistency_tests::candidate` |
| B-002 | keyed digest identity + ops replay/no-alias + immutable path | `cargo test -p helixflow-server ops_routes_tests::idempotency` |
| B-004 | version/init/manual/auto proposal Store transactions | `cargo test -p helixflow-store workspace_initialization_records_tests && cargo test -p helixflow-store proposal_records_apply_tests && cargo test -p helixflow-store proposal_records_auto_apply_tests` |
| B-005 | route barriers + Store CAS | `cargo test -p helixflow-server concurrent_same_base` and focused Store suites |
| B-006, B-007 | reference APIs + candidate coordinator | `cargo test -p helixflow-server version_file_consistency_tests::cleanup`；`cargo test -p helixflow-store version_file_reference_records_tests` |
| B-008 | all read consumers + message ingress | `cargo test -p helixflow-server verified_read`；`cargo test -p helixflow-server workbench_message_graph_tests` |
| B-009 | restore preflight | `cargo test -p helixflow-server version_routes::tests::restore_rejects_corrupt_target` |
| B-010, B-011, B-012 | startup reconciliation/report/crash fixtures | `cargo test -p helixflow-server version_file_reconciliation_tests::startup` |
| B-013 | valid/invalid legacy fixtures | `cargo test -p helixflow-server version_file_reconciliation_tests::legacy` |
| B-014 | five writer integrations | `cargo test -p helixflow-server workspace_routes`；`cargo test -p helixflow-server layout_routes`；`cargo test -p helixflow-server ops_routes_tests::idempotency`；`cargo test -p helixflow-server proposal_routes`；`cargo test -p helixflow-server workbench_message_tests::post_message_auto_applies` |
| B-015 | excluded-path diff + existing collaboration suites | `cargo test -p helixflow-server canvas_collaboration && cargo test -p helixflow-server workspace_canvas` and manifest diff audit |

Cargo test filters may match multiple named tests; implementation必须使用表中的 stable name prefix，不得留下不存在的 aspirational test command。

## 实施顺序

1. 确认 HEAD 含 PR #122，执行三个 inline-test 拆分和 Store version/test 拆分，fresh focused tests 绿。
2. 实现 Store identity/init/reference/manual-message transaction APIs 及 Store fault tests。
3. 实现 exclusive candidate、reference-aware cleanup、verified read 与 startup reconciliation unit/fault tests。
4. 串行接 initial → layout/ops → manual proposal → auto proposal writers。
5. 接 workspace/canvas/direct run/export/restore reads，再接 workbench verified ingress；补 side-effect assertions。
6. 执行 startup/crash/concurrency/full regression、manifest/GH-120 diff审计。

## 风险与回滚

- Security：stored path/hash 错误按 integrity failure 处理；所有公开错误/日志必须脱敏。candidate path 不接受用户原始路径。
- Concurrency：文件与 DB 不可能成为一个跨资源 transaction；正确性依赖 immutable prepublish、SQLite commit truth、精确引用复查和 startup reconciliation 四层契约。
- Compatibility：首次严格对账可能暴露已有损坏并阻止启动，这是预期 fail closed；客户端 message graph mismatch 变为显式 conflict，wire shape 不变。
- Performance：每次 version read增加一次 SHA-256；startup 对 version/proposal 数量线性扫描。

单 PR revert 无 down migration；新 UUID graph path 仍是合法 relative path。回滚不会自动删文件。紧急恢复只能从可信备份修复被引用文件或显式修正 DB/path，禁止通过 silent fallback 关闭 hash 校验。

## 验证

- `cargo fmt --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cd web && npm test`
- `cd web && npm run build`
- `python3 checks/check_workflow.py --repo . --spec-dir specs/GH118`
- `python3 checks/check_workflow.py --repo . --all-specs`
- `git diff --check`
