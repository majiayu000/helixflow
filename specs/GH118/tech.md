# Tech Spec

## Linked Issue

GH-118（https://github.com/majiayu000/helixflow/issues/118）

## Product Spec

见 `specs/GH118/product.md`。

## Planned Changes Manifest

```specrail-planned-changes
{
  "issue": 118,
  "complete": true,
  "paths": [
    "crates/server/src/app_state.rs",
    "crates/server/src/graph_files.rs",
    "crates/server/src/layout_routes.rs",
    "crates/server/src/main.rs",
    "crates/server/src/ops_routes.rs",
    "crates/server/src/ops_routes_tests.rs",
    "crates/server/src/proposal_routes.rs",
    "crates/server/src/run_routes.rs",
    "crates/server/src/version_file_consistency.rs",
    "crates/server/src/version_file_consistency_tests.rs",
    "crates/server/src/version_routes.rs",
    "crates/server/src/workbench_message/gh102_tests.rs",
    "crates/server/src/workbench_message_proposals.rs",
    "crates/server/src/workspace_canvas.rs",
    "crates/server/src/workspace_routes.rs",
    "crates/server/src/workspace_state.rs"
  ],
  "spec_refs": [
    "B-001", "B-002", "B-003", "B-004", "B-005",
    "B-006", "B-007", "B-008", "B-009", "B-010",
    "B-011", "B-012", "B-013", "B-014", "B-015"
  ]
}
```

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| 通用 graph I/O | `crates/server/src/graph_files.rs:18`, `:44`, `:62` | `read_graph_file` 只读/反序列化；`write_json_file` 直接覆盖 final；hash 只在写后返回 | 建立原子发布、verified read 与非静默清理的底座 |
| Layout 写入 | `crates/server/src/layout_routes.rs:72-114`, `:155-160` | 写 `layout-{base_version_id}.json` 后调用 `create_version_after` | 共享目标覆盖是本 issue 的直接复现面；还缺少 transaction 内 pending guard |
| Manual ops 先例 | `crates/server/src/ops_routes.rs:140-175`, `:400-477` | 已有 UUID/temp+rename/CAS/cleanup，但 helper 私有、rename 可覆盖、cleanup 丢弃错误；idempotency 依赖稳定路径 | 提炼为统一实现，同时保留严格幂等语义 |
| 人工 proposal apply | `crates/server/src/proposal_routes.rs:39-86` | applied graph 在 DB transaction 前直接写；DB 失败没有引用复查与清理 | 迁入统一候选生命周期 |
| Agent auto-apply | `crates/server/src/workbench_message_proposals.rs:15-99` | ops/preview/applied 三文件直接写，随后调用原子 proposal/version/message transaction | DB 原子性已有，但文件失败/孤儿未闭环 |
| Version DB transaction | `crates/store/src/lib.rs:204-315`, `crates/store/src/proposal_records.rs:223-329`, `:331-483` | current-version CAS 与 proposal/version/message transaction 已存在 | 保持 DB 为提交真相；文件必须先完整发布，失败后再按引用状态清理 |
| Version reads | `crates/server/src/workspace_state.rs:40-49`, `workspace_canvas.rs:29-39`, `run_routes.rs:56-65`, `version_routes.rs:14-25` | 读取只传 `graph_path`，完全忽略 `graph_hash` | 所有用户可见/执行读取必须统一 verified read |
| Restore | `crates/server/src/version_routes.rs:111-136` | 直接复用目标 `graph_path`/`graph_hash` 创建新 version，不先验证文件 | 防止把损坏历史重新设为 current |
| Startup | `crates/server/src/app_state.rs:34-44` | 迁移后只中断 stale runs、写 provider 状态，无 file/DB reconciliation | 在服务可用前执行严格对账 |
| Initial graph | `crates/server/src/workspace_routes.rs:44-79` | `initial.json` 直接写后创建 version | 纳入统一契约，DB 失败不留不可解释 orphan |

## 设计方案

### 1. 统一候选文件生命周期

新增 `version_file_consistency.rs`，定义私有 `VersionFileCandidate`（workspace、kind、relative final、relative temp、hash、ownership token）。普通候选名为 `<kind>-<uuid_v7>.json`；temp 名还包含独立 nonce。`idempotencyKey` 表示同一逻辑候选身份：发布前计算内容 hash，只有 key/base/hash 都与已提交 version 匹配才复用；同 key 不同内容或尚未被 DB 引用的已占用路径返回冲突，绝不覆盖或删除非本请求文件。

`graph_files.rs` 提供底层：安全路径校验、确定性 JSON bytes、SHA-256、独占 temp 创建、完整写/flush、同文件系统 atomic rename。final 已存在时 fail closed；不得使用会静默替换现有 final 的发布方式。任何写/rename 错误同步清理本请求 temp，清理错误合并到返回错误。

### 2. 文件先行、DB 后置、引用感知清理

各入口统一顺序：内存校验 → stage temp → atomic publish final → DB transaction/CAS → 返回状态。DB 方法继续使用现有 transaction：layout/ops 使用 `create_version_after_without_pending_proposal`；人工 proposal apply 使用 `create_version_after_applying_proposal`；auto-apply 使用 `auto_apply_proposal_version`；初始 graph 使用 `create_version`。

DB 返回错误后，candidate cleanup 先通过现有 `versions_for_workspace` 与 `workspace_proposals` 查询 final/ops/preview 是否被引用：

- 明确未引用：删除本 candidate 的 final/temp；删除失败返回 server error，不吞异常。
- 已引用：保留文件，返回原 DB/响应不明确错误，不把已提交数据删掉。
- 引用查询失败：保留文件并返回包含 cleanup-deferred 原因的错误，由启动对账接管。

候选对象只持有自身 ownership token/path，清理 API 不接受任意路径，从类型边界阻止失败请求删除其他请求的文件。

### 3. 写入入口迁移

- `layout_routes.rs` 移除 `layout-{base}.json`，每请求创建唯一 layout candidate；CAS/pending proposal 在 store transaction 内二次检查。
- `ops_routes.rs` 删除本地 atomic/cleanup helper，复用统一候选；保留 idempotency 行为并加入内容 hash 校验。
- `proposal_routes.rs` 的 applied graph 使用唯一 candidate，DB 失败走引用感知清理。
- `workbench_message_proposals.rs` 的 ops、preview、applied 均 atomic publish；三者作为同一 request-owned candidate set，auto-apply transaction 失败时逐一引用复查后清理。
- `workspace_routes.rs` 初始 graph 采用相同机制；workspace 已创建但 version 失败时返回显式错误并清理未引用 graph，不伪装 workspace 可用。

### 4. Verified version reads

统一 `read_version_graph(data_dir, &VersionRecord)`：安全解析路径，读原始 bytes，计算 SHA-256，使用常量时间无关的精确字符串比较 `sha256:<hex>`，hash 匹配后才反序列化 `WorkflowGraph`。缺失/非法 hash、文件缺失、hash mismatch、非法 JSON 都转为包含 version id/path 类别但不泄漏绝对路径的 server error。

替换 workspace state、canvas、run、layout/ops/proposal base、agent auto-apply base、export 的所有 version read。`create_restore_version` 在 DB CAS 前调用同一 verified read；验证失败不创建 restore version。proposal ops/preview 没有 DB hash，仍使用安全 JSON read，但写入必须 atomic，启动对账至少验证存在与 JSON 类型。

### 5. Startup reconciliation

`AppState::open` 在 Store migration 后、stale-run cleanup 和对外监听前调用 reconciliation：遍历 workspaces 的 versions/proposals，校验所有 version graph 的路径/存在性/hash/JSON，并校验 proposal ops/preview 的安全路径、存在性与 JSON。任何 DB-referenced corruption 形成结构化 `ReconciliationReport` 并使 startup 返回专用 `AppStateError`，服务不监听。

随后只扫描统一命名规则下的 candidate/temp：数据库 set-difference 确认无引用后删除；未知 legacy 文件只计入 `retained_unknown`。报告包含 `verified_versions`、`verified_proposal_files`、`removed_orphans`、`retained_unknown`、`corrupt_references`。删除或引用查询失败也 fail closed，禁止 warning + fallback。

存量路径不重命名、不迁移；已引用且 hash 正确即可继续使用。

### 6. GH-120 边界与集成顺序

GH-118 不修改 `crates/server/src/canvas_collaboration.rs`、comments store/schema、`comments.json`、presence、WebSocket 或 Web comments API/tests。`workspace_canvas.rs` 仅把 version graph 读取替换为 verified helper，不改变 comments 调用。GH-120 当前拥有 comments CAS；两项不得并发编辑同一文件。GH-120 合并后，GH-118 实现必须 rebase 最新 `origin/main`，重新核对 `workspace_canvas.rs` 和 server/store 全量测试，再开始 writable lane。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| B-001 | candidate publish + version write | `cargo test -p helixflow-server version_file_consistency::tests::committed_candidate_hash_matches_db` |
| B-002 | ops idempotency + immutable path | `cargo test -p helixflow-server ops_route_reuses_idempotency_key_without_duplicate_versions`，另测同 key 不同 payload 冲突 |
| B-003 | graph file atomic writer | fault tests：temp write/flush/rename 各失败，断言无 final/version |
| B-004 | route ordering + existing store transactions | layout/ops/proposal DB-trigger failure tests，断言 current/version/proposal/message 全回滚 |
| B-005 | layout/ops/proposal concurrency | 同 base 两请求 barrier 并发，断言 one winner、winner bytes/hash 不变 |
| B-006 | reference-aware cleanup | DB failure + unreferenced 删除；ambiguous/reference-query failure 保留；referenced candidate 不删 |
| B-007 | cleanup error propagation | 注入 remove failure，断言 server error 包含 cleanup 类别且无成功响应 |
| B-008 | all version read consumers | workspace/canvas/run/export 对 missing、invalid JSON、hash mismatch 的负例 |
| B-009 | restore preflight | 损坏 target 后 restore 失败，current/version count 不变 |
| B-010 | startup referenced reconciliation | 缺失 graph、bad hash、bad proposal file 分别使 `AppState::open` 失败 |
| B-011 | startup orphan policy | 仅删除 recognized unreferenced candidate/temp，unknown legacy 保留并计数 |
| B-012 | crash-boundary fixtures | temp-only、final-unreferenced、DB-referenced-missing 三类重启 fixture |
| B-013 | legacy compatibility | legacy path + valid hash 启动和读取通过；missing/invalid hash 被拒绝 |
| B-014 | all writers | initial/layout/ops/manual apply/auto-apply focused route tests |
| B-015 | GH-120 compatibility | `cargo test -p helixflow-server canvas_collaboration` 与 `workspace_canvas` 回归；diff 不含 GH-120-owned files |

## 数据流

写：request → validate/apply in memory → canonical JSON bytes/hash → exclusive temp → atomic final → DB transaction/CAS → verified response read。失败：DB error → query DB references → cleanup only owned+unreferenced / retain on uncertainty → explicit error。读：VersionRecord → safe bytes → SHA-256 compare → JSON decode → consumer。启动：Store open/migrate → reconciliation → stale-run cleanup/provider status → construct AppState → listen。

## 备选方案

- 把 graph JSON 存进 SQLite BLOB：原子性更直接，但引入 schema migration 与大范围存储变更，超出本 issue。
- DB 先提交再写文件：会产生已确认 version 指向缺失文件的窗口，放弃。
- DB 失败无条件删 final：提交结果不明确时会删除已引用成功数据，放弃。
- 只在启动时校验、不在读取时校验：启动后磁盘损坏仍会被返回或运行，放弃。
- 顺带迁移 comments：与 GH-120 所有权冲突且 acceptance surface 不同，放弃。

## 风险

- Security: stored path corruption必须作为 server integrity error；错误不暴露 data-dir 绝对路径。候选路径不接受用户原始路径。
- Compatibility: 首次严格对账可能暴露已有损坏并阻止启动，这是预期 fail-closed；无 schema/API 迁移。
- Performance: 每次 version read 多一次 SHA-256；graph 规模较小。启动扫描与 version/proposal 数量线性相关。
- Maintenance: 文件与 DB 无法形成真正跨资源 transaction，正确性依赖候选所有权、引用复查和启动对账三层契约。
- Concurrency: SQLite operational error 与 commit outcome 不明确时必须保留候选，不能为追求零 orphan 牺牲已提交数据。

## 测试计划

- [ ] Unit：canonical bytes/hash、exclusive temp、atomic publish、no-clobber、verified read、owned cleanup。
- [ ] Fault injection：temp write/flush/rename、DB insert/CAS、引用查询、remove、startup scan 各阶段失败。
- [ ] Concurrency：layout、ops、auto-apply 同 base 竞争，文件内容/DB hash/计数/cleanup 全断言。
- [ ] Integration：workspace/canvas/run/export/restore missing、corrupt、hash mismatch；initial/manual/agent writers。
- [ ] Startup：valid legacy、referenced corrupt、recognized orphan、unknown legacy fixtures。
- [ ] Full：`cargo fmt --check`、`cargo check --workspace`、`cargo test --workspace`、Web 回归与 SpecRail gates。

## 回滚方案

单 PR revert 恢复旧写入/读取逻辑；不需要 down migration。回滚不会删除新 UUID graph 文件，versions 仍保存兼容相对路径与 hash。若需紧急恢复服务，应先从可信备份修复被对账识别的已引用文件，不得用关闭 hash 校验的 silent fallback 作为常规开关。
