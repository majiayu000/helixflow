# Task Plan

## Linked Issue

GH-154

## Spec Packet

- Product: `specs/GH154/product.md`
- Tech: `specs/GH154/tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP154-T0` | test/contracts | none | 固化 restart recovery 状态矩阵、三类 dispatch failure 和五个 crash-point fixtures；覆盖 queued/estimating/waiting/running、dispatching/active/terminal、Atlas/fal/mock。 | 当前实现可稳定复现 handle 丢失；not-submitted/rejected/unknown 可区分；fixture 不触发真实付费调用且无 secret。 | `cargo test -p helixflow-run --locked recovery_baseline` |
| `SP154-T1` | store | `T0` | 增加带 dispatch owner/origin/scope/deadline、result_ready/spool 且 step 唯一的 provider tasks，以及 outputs/cost/lease/execution/terminalization/continuation/artifact journal API。 | live dispatch 不被 abandon；result_ready+actual cost 先于 artifact；旧 DB 可升级；terminal/continuation/journal CAS、FK/cascade 通过。 | `cargo test -p helixflow-store --locked run_recovery` |
| `SP154-T2` | gateway | `T1` | 将 Provider 生命周期拆为 typed dispatch/resume/cancel；Atlas/fal 按 origin+scope 恢复；mock 确定恢复；逐个清洗 `outputs[*].ArtifactPayload.meta`。 | correctness path 无 `in_flight` truth；三类 dispatch 正确；scope/恶意 URL 在请求前拒绝；artifact/API 无 task id、key、header、完整 URL。 | `cargo test -p helixflow-gateway --locked recovery` |
| `SP154-T3` | run | `T1`,`T2` | executor 写唯一 dispatch intent/owner/handle；provider terminal 先持久化 result_ready/spool/cost，再 materialize；其余 typed step finalizer、DAG、settler、continuation 与 artifact journal。 | interrupt 等 live dispatch；result download failure 不误判 active/abandoned且 cost 不丢；preflight/queued/sibling/retry/orphan 幂等。 | `cargo test -p helixflow-run --locked recovery` |
| `SP154-T4` | server/run | `T3` | startup 穷尽 queued/running/所有 terminalization/continuation/journal 并 lease claim；按绝对 deadline 恢复/补取消；保留 queued/estimating/running online interrupt。 | deadline 不因重启延长；interrupt settler 可跨 crash；all-queued/builtin/无 handle 显式收敛；recovered failed self-heal exactly once。 | `cargo test -p helixflow-server --locked restart_recovery && cargo test -p helixflow-run --locked self_heal` |
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
- live dispatch owner 与 interrupt settler 并发，Accepted 后 handle 落库；
- provider result_ready/cost commit 后、artifact download/publish 前退出与重复失败；
- online interrupt 后、dispatch handle 返回/CAS 前退出；
- active handle commit 后退出；
- artifact file 写完、DB finalizer 前退出；
- artifact journal publish 后、commit/GC 前退出；
- terminal transaction commit 后、EventBus publish 前退出；
- 两个 recovery owner 同时 claim/renew；
- dispatch local not-submitted、上游明确 rejected、timeout/断线 outcome-unknown；
- fal callback URL origin、userinfo、fragment、secret query tampering；
- Atlas/fal provider account、API origin 和 credential scope 漂移；
- Atlas recovery timeout 且 cancel unsupported；
- cache hit 后、下游启动前退出；
- 非最终 poll-complete 后 online interrupt、最终 poll-complete/interrupt/cancel 三方竞态；
- sync dispatching→completed、async active→completed、builtin/cache 无 task row finalizer；
- artifact hydration/API 序列化不含 task id 或远端 URL；
- queued complete intent/estimate、partial estimate、fingerprint mismatch；
- running all-queued、builtin restart-safe/unsafe、provider running 无 handle、未知组合；
- parallel sibling completed/cancelled/abandoned 与 terminalization work-item restart；
- queued/estimating/running interrupt 与 dispatching/active/result_ready settler restart；
- failed-settling 与 user interrupt 并发，interrupt 胜出且不 self-heal；
- failed-ready 与 user interrupt 并发，interrupt 胜出且不 self-heal；
- outcome-unknown、missing/invalid handle、deadline abandon 的 parallel sibling settlement；
- restart-unsafe builtin、workspace active-run conflict loser 的 parallel sibling settlement；
- failed commit→continuation claim、retry child insert→linkage commit；
- 含历史重复 `(parent_run_id,attempt)` rows 的 migration；
- recovery deadline 跨连续多次 restart；
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
