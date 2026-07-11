# Task Plan

## Linked Issue

GH-102 (#102)

## Spec Packet

- Product: `specs/GH102/product.md`
- Tech: `specs/GH102/tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP102-T1 | primary agent | none | 在 `crates/store/src/proposal_records.rs` 增加原子自动应用事务及返回类型，一次提交 proposal applied、version、workspace current、`proposal_applied` message 和关联 id | 成功路径引用完整；version conflict/任一步失败时 DB 不出现 proposal/version/message 半状态，current version 不变 | `cargo test -p helixflow-store proposal` |
| SP102-T2 | primary agent | SP102-T1 | 重构 `crates/server/src/workbench_message.rs`，把 agent create/modify/debug turn 接到原子事务；提取小模块控制文件体积；日志在事务前持久化；proposal response 不创建 run | 合法 proposal 自动应用并返回 version message；失败后下一请求不受 pending proposal 阻塞；`run=None` | `cargo test -p helixflow-server workbench_message` |
| SP102-T3 | primary agent | none | 在 `crates/server/src/sweep_support.rs` 建立普通 run 与 sweep 共用的成本确认策略；缺失阈值默认 0，无效/负数/非有限配置明确报错 | USD `<`/`=`/`>`、非 USD、非有限值和 env missing/invalid 行为符合 tech spec | `cargo test -p helixflow-server confirmation_threshold` |
| SP102-T4 | primary agent | SP102-T3 | 普通 agent run request 在 request/estimated-ledger 后按策略分流；阈值内复用 `start_confirmed_run`，超阈值保留现有 pending confirmation | auto-start 与 confirm-start 都使用同一状态抢占/事件/ledger；重复确认不双启动 | `cargo test -p helixflow-server run_request`；`cargo test -p helixflow-run confirm` |
| SP102-T5 | primary agent | SP102-T3 | seed sweep 在 group estimate 后按同一策略分流；阈值内复用 `start_confirmed_sweep`，超阈值保留 group confirmation | 整组只启动一次；recommended run payload、estimated/actual ledger 与等待确认路径正确 | `cargo test -p helixflow-server seed_sweep`；`cargo test -p helixflow-run sweep` |
| SP102-T6 | primary agent | SP102-T2, SP102-T4, SP102-T5 | 更新 Web state/文案与 README/runtime/prompt 文档，保留历史 pending proposal 兼容 UI；记录 GH-91 决策变化和阈值部署契约 | `proposal_applied` 清理 preview；run panel 区分自动启动/等待确认；文档明确 proposal 不隐式起跑与 env 行为 | `npm test`；`rg -n "HELIXFLOW_AGENT_RUN_CONFIRMATION_THRESHOLD_USD|proposal_applied" README.md docs web/src` |
| SP102-T7 | primary agent | SP102-T1..T6 | 补足 store/server/Web 回归，运行全量构建、测试、SpecRail gate 和 diff 检查；修复 warning，不弱化断言 | 产品 10 条不变量都有确定性证据；fresh full suite 全绿；无新增 warning/format/diff 错误 | `cargo fmt --check && cargo check --workspace && cargo test --workspace`；`cd web && npm test && npm run build`；`python3 checks/check_workflow.py --repo . --spec-dir specs/GH102`；`git diff --check` |

## 并行拆分

本 PR 采用单一 writable lane。`proposal_records.rs`、`workbench_message.rs`、`sweep_support.rs` 与共享 server tests 存在串行依赖，不启动并行写代理，避免违反 W-14。

允许一个独立只读 reviewer lane 在 SP102-T7 后检查 product-to-test coverage、事务边界、成本闸门和 diff scope；reviewer 不修改文件。

## 验证

- `python3 checks/route_gate.py --repo . --route implement --issue 102 --state ready_to_implement --evidence <issue-evidence> --json`
- `cargo fmt --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cd web && npm test`
- `cd web && npm run build`
- `python3 checks/check_workflow.py --repo . --spec-dir specs/GH102`
- `git diff --check`
- PR reviewer evidence + current CI + reviewThreads + `pr_gate` before merge

## Handoff Notes

- GH-102 是 GH-101 的前置依赖；GH-102 未合并前不发布 GH-101 PR。
- 本地候选提交 `17ce6fc` 只作为实现起点，必须修复 reviewer 发现的 pending proposal 残留，并按本 spec 补足阈值配置与事务测试后才可提交。
- 自动应用 proposal 不自动创建 run。成本阈值仅用于独立的普通 agent run request 和 seed sweep。
- 保留人工 proposal apply/dismiss routes 兼容历史 pending 数据。
- 不 force push；根工作区的 UI commits/dirty files 不属于本 PR。
