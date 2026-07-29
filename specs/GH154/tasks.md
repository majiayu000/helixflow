# Task Plan

## Linked Issue

GH-154

## Spec Packet

- Product: `specs/GH154/product.md`
- Tech: `specs/GH154/tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP154-T0` | test/contracts | none | 固化 restart recovery 状态矩阵和五个 crash-point fixtures；覆盖 queued/estimating/waiting/running、dispatching/active/terminal、Atlas/fal/mock。 | 当前实现可稳定复现 handle 丢失；fixture 不触发真实付费调用且无 secret。 | `cargo test -p helixflow-run --locked recovery_baseline` |
| `SP154-T1` | store | `T0` | 增加 `run_provider_tasks`、`run_step_outputs`、cost operation key、`run_recovery_leases` migration 和 typed Store API。 | 重开 DB 可查询 handle；状态 CAS、lease、output/cost replay、terminal transaction 与 FK/cascade 测试通过。 | `cargo test -p helixflow-store --locked run_recovery` |
| `SP154-T2` | gateway | `T1` | 将 Provider 生命周期拆为 dispatch/resume/cancel；Atlas 返回/恢复 prediction handle 且 cancel 仍 unsupported；fal 返回/恢复/取消经双重 URL 校验的 handle；mock 提供确定性测试恢复。 | correctness path 无 `in_flight` truth；恶意 URL 在发请求前拒绝；日志/event fixture 无 key、header、完整 URL。 | `cargo test -p helixflow-gateway --locked recovery` |
| `SP154-T3` | run | `T1`,`T2` | executor 在 submit 前写 dispatching，handle CAS active；实现幂等 step finalizer、durable output reconstruction、剩余 DAG continuation 和 DB-based online interrupt。 | 五个 crash window 不重提；重复 resume 不复制 artifact/cost/event；正常 interrupt 语义回归。 | `cargo test -p helixflow-run --locked recovery` |
| `SP154-T4` | server/run | `T3` | AppState 严格解析 queued requeue flag；startup 分类/lease claim 后后台恢复，续租、退避、补取消、abandoned 风险；normal/recovered 共用 failure finalizer。 | 启动不等待远端；queued 默认 interrupted、true 才 requeue、invalid 阻止启动；recovered failed 进入现有 self-heal 一次。 | `cargo test -p helixflow-server --locked restart_recovery && cargo test -p helixflow-run --locked self_heal` |
| `SP154-T5` | frontend | `T4` | Web 处理 recovery/requeue/risk events，保留 durable notice 并在 terminal/recovery 事件后 refetch。 | WebSocket、seq gap、刷新、workspace switch 下状态一致；abandoned/cancel failure 显示计费风险且不展示 secret。 | `cd web && npx tsc --noEmit && npm test -- --run store-events store-background && npm run build` |
| `SP154-T6` | coordinator | `T0`–`T5` | 全量 crash matrix、并发 lease、正常执行/cost/sweep/interrupt/self-heal 回归，核对 #153 handoff 与 PR evidence。 | 所有 invariant 有 fresh evidence；无真实 billable call；exact-head review 无 actionable finding。 | `cargo fmt --all -- --check && cargo check --workspace --locked && cargo test --workspace --locked` |

## 实现顺序与文件所有权

1. `SP154-T0` 先固定失败窗口和 provider fixtures。
2. `SP154-T1` 完成后，`SP154-T2` 才能稳定返回持久化 DTO；避免 gateway 自己打开
   SQLite 或复制 Store。
3. `SP154-T3` 统一 normal execution 与 resume 的 step finalizer。
4. `SP154-T4` 只负责编排 startup/lease/failure handoff，不复制 executor。
5. `SP154-T5` 在稳定 event contract 后接入。

若并行：

- store lane 只改 `crates/store/migrations/**` 和新/相关 store record/test 文件；
- gateway lane 只改 `crates/gateway/**`；
- run lane 只改 `crates/run/**`；
- server lane 只改 `crates/server/src/app_state.rs`、workspace event/startup recovery 测试；
- frontend lane 只改 `web/src/store-events.ts` 及其测试。

`crates/run/src/lib.rs`、`crates/store/src/lib.rs`、`crates/gateway/src/lib.rs`、
`crates/server/src/app_state.rs` 等共享入口在同一时刻只能由一个 owner 修改；
`Cargo.lock` 由 coordinator 最后统一处理。

## 验证

实现完成后按顺序运行：

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test -p helixflow-store --locked run_recovery
cargo test -p helixflow-gateway --locked recovery
cargo test -p helixflow-run --locked recovery
cargo test -p helixflow-server --locked restart_recovery
cargo test --workspace --locked
cd web
npm ci
npx tsc --noEmit
npm test -- --run
npm run build
cd ..
git diff --check
```

当前 main 已在 `a1ee3eb` / `#157` 退役 repo-local SpecRail automation，
`checks/check_workflow.py` 不存在，因此它不是本规格的可执行 gate。规格文件是设计记录；
implementation PR 仍须以 exact-head review、fresh GitHub Actions 和上述构建/测试为准。

此外必须执行非真实付费的 fault injection：

- dispatch intent commit 后退出；
- provider 已接受、handle CAS 前退出；
- active handle commit 后退出；
- artifact file 写完、DB finalizer 前退出；
- terminal transaction commit 后、EventBus publish 前退出；
- 两个 recovery owner 同时 claim/renew；
- fal callback URL origin、userinfo、fragment、secret query tampering；
- Atlas recovery timeout 且 cancel unsupported；
- queued flag missing/false/true/invalid。

## Handoff Notes

- #154 是 #153 的前置；本任务只统一 recovered-failed finalizer，不实现 Agent 修图。
- Atlas 只有 resume poll，没有 cancel；任何 Atlas cancel 实现都违反 #124/#154。
- `dispatching` 无 handle 是 unknown，不是“尚未提交”的证明；Atlas/fal 不得自动重发。
- DB 中的 task record、output mapping 和 lease 是 truth；gateway 内存 map 不能作为
  fallback。
- `HELIXFLOW_RUN_REQUEUE_ON_RESTART` 默认 false 且严格解析；waiting confirmation
  永不自动执行。
- full URL/task id 仅限 store/gateway 内部，禁止进入 event、API、Web、日志或 Agent
  context。
