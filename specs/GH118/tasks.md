# Task Plan

## Linked Issue

GH-118（https://github.com/majiayu000/helixflow/issues/118）

## Spec Packet

- Product: `specs/GH118/product.md`
- Tech: `specs/GH118/tech.md`
- Baseline: latest `origin/main` containing PR #121/#122 (`0471c213` at this revision)

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Concrete Verify | Covers |
| --- | --- | --- | --- | --- | --- | --- |
| SP118-T0 | backend consistency lane | clean rebase on latest main | 先把 `workspace_state.rs`、`run_routes.rs`、`workbench_message.rs` 的 inline tests 原样拆到 manifest 指定的三个 `*_tests.rs`；把 `lib.rs` 的 version methods/tests 拆到 `version_records.rs`/`version_records_tests.rs`；只做机械移动并保持 GH60/GH102 test 可见性 | 四个 production files 均远离 800 行；拆分前后测试集合与行为不变；diff 不含 comments owner files | `cargo test -p helixflow-store version_records_tests && cargo test -p helixflow-server workspace_state_tests && cargo test -p helixflow-server run_routes_tests && cargo test -p helixflow-server workbench_message_tests && cargo fmt --check && git diff --check` | B-015 |
| SP118-T1 | backend consistency lane | SP118-T0 | 在 Store 新增无 DB 副作用的 `reserve_workspace_identity`、单 transaction `create_workspace_with_initial_version`、version/proposal path reference APIs；扩展人工 proposal apply transaction，使 version/current/proposal/applied message all-or-none | 预分配 IDs 可供 path/reference query；initial 三项与 manual proposal 四项无半提交；`lib.rs` 仅注册/reexport共用类型 | `cargo test -p helixflow-store workspace_initialization_records_tests && cargo test -p helixflow-store version_file_reference_records_tests && cargo test -p helixflow-store proposal_records_apply_tests && cargo test -p helixflow-store proposal_records_auto_apply_tests && cargo check -p helixflow-store` | B-004, B-005, B-006, B-007, B-014 |
| SP118-T2 | backend consistency lane | SP118-T1 | 在 `graph_files.rs` + `version_file_consistency.rs` 实现 canonical bytes/hash、owned UUID candidate set、`create_new` temp、sync、same-filesystem hard-link no-replace publish、parent sync、verified `VersionRecord` read、引用感知 cleanup；在 `main.rs` 注册模块 | ordinary overwrite rename 不存在；candidate 只能清自身；缺失/非法 hash/JSON/path均结构化 fail closed | `cargo test -p helixflow-server version_file_consistency_tests::candidate && cargo test -p helixflow-server version_file_consistency_tests::cleanup && cargo test -p helixflow-server version_file_consistency_tests::verified_read && cargo fmt --check` | B-001, B-002, B-003, B-006, B-007, B-008, B-013 |
| SP118-T3 | backend consistency lane | SP118-T2 | 串行接 initial workspace、layout、manual ops writers；initial 用预分配 identity + 单 Store transaction，layout 用 pending-guard CAS；ops unkeyed 用 UUID，keyed 用 workspace+base+raw key 的域分隔 digest稳定路径，verified base 后按 path/workspace/parent/hash 严格 replay | 三入口都是 publish → Store truth；同 base 至多一 winner；raw key不进路径；同 key同 payload可 replay；同 key不同 payload conflict；不同 key同内容不按 content alias；失败 cleanup不误删 | `cargo test -p helixflow-server workspace_routes && cargo test -p helixflow-server layout_routes && cargo test -p helixflow-server ops_routes_tests::idempotency && cargo test -p helixflow-server concurrent_same_base` | B-001, B-002, B-004, B-005, B-006, B-007, B-014 |
| SP118-T4 | backend consistency lane | SP118-T2, SP118-T1 | 接人工 proposal applied candidate 与 agent auto-apply ops/preview/applied candidate set；人工 route 删除 transaction 后独立 message；两路 base 都用 verified read | manual version/current/proposal/message 同 transaction；auto 四类 DB record 保持 all-or-none；多文件失败逐一引用复查 | `cargo test -p helixflow-server proposal_routes && cargo test -p helixflow-server workbench_message_tests::post_message_auto_applies && cargo test -p helixflow-store proposal_records_apply_tests && cargo test -p helixflow-store proposal_records_auto_apply_tests` | B-001, B-004, B-005, B-006, B-007, B-008, B-014 |
| SP118-T5 | backend consistency lane | SP118-T2, SP118-T0 | workspace state/canvas、direct run、layout/ops/proposal base、export/restore 全接 `read_version_graph`；新增 `workbench_message_graph`，在任何 message/agent/run side effect 前核对 current/base、verified server graph 与 client graph，后续 chat/proposal/run/sweep只用 server graph | 所有 VersionRecord consumer 对 missing/bad hash/bad JSON fail closed；restore不推进；stale/tampered message不写 user message、不调用 agent、不创建 run | `cargo test -p helixflow-server verified_read && cargo test -p helixflow-server version_routes && cargo test -p helixflow-server workbench_message_graph_tests && cargo test -p helixflow-server run_routes_tests` | B-008, B-009, B-013, B-014 |
| SP118-T6 | backend consistency lane | SP118-T1, SP118-T2, SP118-T4 | 在 `AppState::open` 的监听前阶段实现 version/proposal reconciliation；仅删 recognized + 双查询无引用候选；成功报告留在 AppState并由 `main` 输出单行 JSON，失败 error/log 脱敏 | referenced corruption/scan/delete/query failure阻止启动；unknown legacy保留；报告字段、计数、路径类别可断言且无绝对路径/内容/SQL | `cargo test -p helixflow-server version_file_consistency_tests::startup && cargo test -p helixflow-server version_file_consistency_tests::legacy && cargo test -p helixflow-server app_state` | B-010, B-011, B-012, B-013 |
| SP118-T7 | backend consistency lane | SP118-T3..T6 | 补齐 tech fault table 的 CandidateIo、SQLite TEMP trigger、commit-then-error、Barrier concurrency、crash-boundary fixtures；每个负例断言 DB/current/file/message/proposal side effects | tech fault/race 表每行有 stable test prefix；winner bytes/hash与 DB一致；unknown outcome保留不猜删；无测试断言弱化 | `cargo test -p helixflow-store workspace_initialization_records_tests && cargo test -p helixflow-store version_file_reference_records_tests && cargo test -p helixflow-store proposal_records_apply_tests && cargo test -p helixflow-store proposal_records_auto_apply_tests && cargo test -p helixflow-server version_file_consistency_tests && cargo test -p helixflow-server concurrent_same_base && cargo test -p helixflow-server workbench_message_graph_tests` | B-001, B-003, B-004, B-005, B-006, B-007, B-008, B-009, B-010, B-011, B-012, B-014 |
| SP118-T8 | coordinator | SP118-T7 | 做 source-of-truth、manifest、兼容路径、GH-120 边界与全量回归审计；确认 test commands确实匹配测试且无 checked implementation box | B-001..B-015 全覆盖；fresh full suite通过；diff只含 manifest paths且不含 PR122 comments files | `cargo fmt --check && cargo check --workspace && cargo test --workspace && (cd web && npm test && npm run build) && python3 checks/check_workflow.py --repo . --spec-dir specs/GH118 && python3 checks/check_workflow.py --repo . --all-specs && git diff --check` | B-001, B-002, B-003, B-004, B-005, B-006, B-007, B-008, B-009, B-010, B-011, B-012, B-013, B-014, B-015 |

所有 task 都处于待实现状态；本 spec revision 不声称任何 production checkbox 已完成。

## 依赖与串行边界

单一 writable lane 独占 Planned Changes Manifest 全部 code/test 路径，避免 graph I/O、Store transaction、startup 与 routes 在共享文件上违反 W-14。只读 reviewer 可在 SP118-T7 后核对 transaction ordering、server-graph trust、cleanup ownership、structured redaction 与 GH-120 排除边界，不修改文件。

依赖主链：`T0 → T1 → T2 → (T3, T4, T5) → T6 → T7 → T8`。虽然 T3/T4/T5 在逻辑上可分组，仍由同一 lane 串行修改，因为它们共享 candidate helper、Store APIs 与 route fixtures。若 latest main 不再包含 PR #122，或 manifest 任一路径与其他 active writable lane 重叠，暂停实现并重新 preflight，不自行扩权。

## 验证清单

- Product invariant union：task `Covers` 必须完整覆盖 `B-001`..`B-015`。
- Manifest diff：实现 diff 必须是 tech manifest 子集；必须显式确认不含 GH-120 comments migration/store/server/Web files。
- Stable filters：逐条运行 task 的 Concrete Verify，确认没有 0 tests matched。
- Fresh build/test：`cargo fmt --check`、`cargo check --workspace`、`cargo test --workspace`、`cd web && npm test && npm run build`。
- SpecRail：`python3 checks/check_workflow.py --repo . --spec-dir specs/GH118` 与 `--all-specs`。
- Hygiene：`git diff --check`。

## Handoff Notes

- SQLite `VersionRecord` + workspace current pointer 是唯一 commit truth；filesystem candidate 是 adapter，不是并列真相源。
- 初始 workspace 使用无 DB 副作用的预分配 workspace/version identity；candidate path workspace-scoped；一个 Store transaction 插 workspace/version/current，commit unknown 后能按预分配 version/path精确复查。
- 人工 proposal 当前确实在 Store transaction 后另建 applied message；GH-118 必须把 message 创建并入 `create_version_after_applying_proposal` transaction。
- workbench message 当前确实把客户端 graph 传给 classification/agent/run；GH-118 保留 wire field，但 server verified graph才是实际输入，client graph只作等值校验/selection context。
- legacy version path 和 proposal ops/preview 的 converge/delete 条件见 tech compatibility table；GH-118 不删除 referenced 或 unknown legacy 文件。
- 不修改、不迁移、不清理 `comments.json` 或 PR #122 comments SQLite truth；不 force push；spec-only revision 不开 PR、不推远端。
