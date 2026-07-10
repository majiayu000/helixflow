# Task Plan

## Linked Issue

GH-101 (#101)

## Spec Packet

- Product: `specs/GH101/product.md`
- Tech: `specs/GH101/tech.md`

## Implementation Tasks

| ID | Owner | Dependencies | Task | Done When | Verify |
|----|-------|--------------|------|-----------|--------|
| SP101-T1 | agent | none | store: `runs` 表加 `parent_run_id`、`attempt` 列 + migration；新增 `create_retry_run(parent, attempt)` 派生新 run 记录 | 列与方法存在，同步所有 RunRecord 构造点（U-26） | `cargo test -p helixflow-store` |
| SP101-T2 | agent | SP101-T1 | run: 在 `start_confirmed_run` 后台 spawn 失败分支实现有界自愈循环，读 `HELIXFLOW_RUN_MAX_RETRIES`（默认 1），每次 `append_run_event("run.retry")`，耗尽保留 `failed`+`error_json` | 失败 run 自动派生重跑 ≤N 次，末次终态 failed | `cargo test -p helixflow-run retry` |
| SP101-T3 | agent | SP101-T2 | run/server: 自愈重跑前经 `requires_run_confirmation`；>阈值挂 `waiting_confirmation` 不自动起 | 超阈值重跑返回 pending_confirmation | `cargo test -p helixflow-server retry_over_threshold` |
| SP101-T4 | agent | none | store: `ArtifactRecord` 加 `review_state`（默认 pending）+ migration + 回填历史为 accepted；新增 `set_artifact_review_state`；同步 `NewArtifact`/`persist_artifact` | 列与方法存在，新 artifact 默认 pending | `cargo test -p helixflow-store artifact_review` |
| SP101-T5 | agent | SP101-T4 | server: `artifact_routes.rs` 加 `POST /outputs/{id}/accept`、`/reject`（仿 select_output，尊重 latest_output_scope），注册 main.rs | accept/reject 流转 review_state；跨旧 run 409；accepted 再改 409 | `cargo test -p helixflow-server accept_reject` |
| SP101-T6 | agent | SP101-T5, SP101-T2 | server: `reject` 带 `rerun=true` 时经统一基座 `start_confirmed_run` 触发重跑 | reject+rerun 起一次新 run | `cargo test -p helixflow-server reject_with_rerun` |
| SP101-T7 | agent | SP101-T2, SP101-T6 | web: 适配 run.retry 事件展示 + artifact accept/reject 按钮（复用现有 chat-pane/run-panels 模式） | UI 显示重试与审查动作 | `npm run test`（web） |
| SP101-T8 | agent | all | 文档：新 env（MAX_RETRIES、阈值）记入部署文档；docs 记录 GH91 决策反转 | 部署文档含两个 env | `python3 checks/check_workflow.py --repo . --spec-dir specs/GH101` |

## Parallel Split（并行拆分）

- Lane A（自愈）：SP101-T1 → T2 → T3。文件：`crates/store/src/run_records.rs`、`crates/run/src/cost_gate.rs`、`crates/run/src/executor.rs`。
- Lane B（审查）：SP101-T4 → T5。文件：`crates/store/src/run_records.rs`（review 列）、`crates/server/src/artifact_routes.rs`、`crates/server/src/main.rs`。
- 汇合：SP101-T6（reject 触发重跑，依赖两 lane）→ T7（web）→ T8（docs）。
- W-14 注意：两 lane 都改 `crates/store/src/run_records.rs`——**不可并行写同文件**，T1/T4 需串行或由单一 owner 顺序改。

## Verification

- `cargo test -p helixflow-store && cargo test -p helixflow-run && cargo test -p helixflow-server`（现 77 server 测试须仍全绿）。
- `python3 checks/check_workflow.py --repo . --spec-dir specs/GH101` 通过。
- 手动：构造 provider 失败 run 观察自愈；生成输出后 accept/reject。

## Handoff Notes

- 本 spec 是**人工审批 gate**：批准并把 issue #101 置 `ready_to_implement` 后才开始实现。
- 反转 GH91 非目标已在 tech.md 记录，实现时须在 docs 留痕。
- 已实现的自动应用+成本闸门（77 测试通过）不在本 spec 改动范围，只在其上补两块。
- issue #101 尚未创建；若创建后号不同，重命名 `specs/GH101/` 并同步文内 token。
