# Task Plan

## Linked Issue

GH-118（https://github.com/majiayu000/helixflow/issues/118）

## Spec Packet

- Product: `specs/GH118/product.md`
- Tech: `specs/GH118/tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify | Covers |
| --- | --- | --- | --- | --- | --- | --- |
| SP118-T1 | backend consistency lane | GH-120 merged + rebase | 在 `graph_files.rs` 与新 `version_file_consistency.rs` 建立 canonical bytes/hash、unique candidate、exclusive temp、atomic no-clobber publish、ownership token、verified read 和引用感知 cleanup；在 `main.rs` 注册模块 | final 永不覆盖；任何 file/cleanup 错误显式返回；candidate 只能清理自身 | `cargo test -p helixflow-server version_file_consistency` | B-001, B-002, B-003, B-006, B-007 |
| SP118-T2 | backend consistency lane | SP118-T1 | 将 initial/layout/ops 写入接到统一候选生命周期；layout 使用 transaction 内 pending guard；ops 保留并加强 idempotency hash 语义，删除私有 atomic/吞错 cleanup helper | 三入口都按 file publish → DB commit 顺序；同 key 不同内容冲突；无共享 base 文件 | `cargo test -p helixflow-server layout_routes`；`cargo test -p helixflow-server ops_route` | B-002, B-004, B-005, B-014 |
| SP118-T3 | backend consistency lane | SP118-T1 | 将人工 proposal apply 与 agent auto-apply 的 applied/ops/preview 文件接到 candidate set；复用既有 store transaction，失败逐文件引用复查 | proposal/version/message 保持全事务；DB 失败不留未解释文件且不误删已引用文件 | `cargo test -p helixflow-server proposal_routes`；`cargo test -p helixflow-server gh102_tests` | B-001, B-004, B-005, B-006, B-014 |
| SP118-T4 | backend consistency lane | SP118-T1 | workspace state、canvas、run、layout/ops/proposal base、auto-apply base、export 全部改用 verified version read；restore commit 前验证 target | 所有 version 消费方对 missing/invalid/hash mismatch fail closed；restore 不推进损坏目标 | `cargo test -p helixflow-server version_file_consistency_tests::verified_reads` | B-008, B-009, B-013 |
| SP118-T5 | backend consistency lane | SP118-T1, SP118-T3 | 在 `AppState::open` 加启动 reconciliation 和结构化报告；校验 version/proposal 引用，清理 recognized unreferenced candidate/temp，保留 unknown legacy | referenced corruption 阻止启动；安全 orphan 被清；unknown 不删；错误不降级 | `cargo test -p helixflow-server version_file_consistency_tests::startup_reconciliation` | B-010, B-011, B-012, B-013 |
| SP118-T6 | backend consistency lane | SP118-T2..T5 | 补齐 fault-injection、barrier concurrency 与 crash-boundary tests；使用 SQLite trigger/受控 filesystem fixture 注入 DB/file 故障，不弱化既有断言 | 每个 tech mapping 有确定性正/负例；winner bytes/hash 与 DB 精确一致；loser 无污染 | `cargo test -p helixflow-server version_file_consistency_tests`；`cargo test -p helixflow-server ops_route` | B-001, B-003, B-004, B-005, B-006, B-007, B-008, B-009, B-010, B-011, B-012, B-014 |
| SP118-T7 | coordinator | SP118-T6 | GH-120 边界审计、全量构建/测试、spec 对照与 diff 检查；确认未修改 comments owner 文件 | product B-001..B-015 全覆盖；fresh full suite 通过；diff 不含 `canvas_collaboration.rs`/comments schema/Web comments | `cargo fmt --check && cargo check --workspace && cargo test --workspace && (cd web && npm test && npm run build) && python3 checks/check_workflow.py --repo . --spec-dir specs/GH118 && git diff --check` | B-001, B-002, B-003, B-004, B-005, B-006, B-007, B-008, B-009, B-010, B-011, B-012, B-013, B-014, B-015 |

## 并行拆分

本 issue 使用单一 writable lane，避免 graph I/O、startup 与各 route 在共享文件上违反 W-14。建议该 lane 独占 tech manifest 中全部 code/test 路径；允许一个只读 reviewer 在 SP118-T6 后核对 transaction ordering、hash coverage、cleanup ownership 与 GH-120 边界，不修改文件。

GH-120 先合并。GH-118 lane 开始前必须 rebase 最新 `origin/main`，确认 `crates/server/src/canvas_collaboration.rs` 及 comments store/schema 仅由 GH-120 所有；GH-118 不触碰它们。若 GH-120 同时改动 `workspace_canvas.rs`，先串行重放 GH-120，再仅在最终文件上修改 version graph read 调用，禁止两个 writable lane 并发。

## 验证

- Product invariant set：`B-001`..`B-015`；task `Covers` union 必须完全包含该集合。
- `cargo fmt --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cd web && npm test`
- `cd web && npm run build`
- `python3 checks/check_workflow.py --repo . --spec-dir specs/GH118`
- `python3 checks/check_workflow.py --repo . --all-specs`
- `git diff --check`

## Handoff Notes

- 实现基线必须晚于 GH-120 合并并重新核对；当前 spec 基于 `origin/main` `8276fa2`。
- DB transaction 继续以现有 Store 方法为真相，不新增 migration；本 issue 修正的是 file publish、调用顺序、失败清理、read verification 与 startup reconciliation。
- `comments.json`、comments sequence/CAS/idempotency、presence、WebSocket、Web comments 均不在本 issue。
- 失败清理的首要安全目标是不误删已引用成功文件；引用状态不明时允许显式保留 orphan，交由启动对账处理，禁止猜测删除。
- 不 force push；不在 spec-only 阶段开始 production/test code、开 PR 或 merge。
