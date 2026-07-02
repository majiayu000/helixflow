# Task Plan

## Linked Issue

GH-58

## Spec Packet

- Product: `specs/GH58/product.md`
- Tech: `specs/GH58/tech.md`

## 实现任务

- [ ] `SP58-T1` store 清扫查询与事务更新。Owner: store 线程。Done when: `interrupt_stale_active_runs()` 在单事务内把 `queued`/`estimating`/`running` run 置为 `interrupted`、其非终态步骤置为 `skipped` 并返回受影响 run,`waiting_confirmation` 不受影响,含单元测试。Verify: `cargo test -p helixflow-store`
- [ ] `SP58-T2` run crate 执行模型两阶段拆分。Owner: run 线程。Done when: `start_manual_run` / `start_confirmed_run` / `start_confirmed_sweep` 在同步阶段完成建档/认领、于 run 记录创建同刻注册 interrupt 条目(先于任何 estimating/执行动作)、持久化 queued 快照并构造完整 `RunOutcome` 后才 `tokio::spawn` 执行;估算阶段 provider 调用纳入 `tokio::select!` 中断分支,estimating 期间 interrupt 收敛 `interrupted`;sweep confirm 认领为 `queued`,后台轮到成员时才置 `running`(组内任意时刻至多一个 `running`);旧同步入口删除,后台失败以 error 级日志 + `run.failed` 事件收敛,queued 阶段与执行中中断均收敛 `interrupted`,含单元测试。Verify: `cargo test -p helixflow-run`
- [ ] `SP58-T3` server 路由与启动清扫接线。Owner: server 线程。依赖 SP58-T1、SP58-T2。Done when: queue/confirm 端点调用新两阶段接口并立即返回 run_id 与进行中快照(sweep confirm 成员为 `queued`),`AppState::open` 启动时调用清扫并为受影响 run 追加 `run.interrupted` 事件,interrupt/hold 语义与并发防护保持("取第一个 running"的 sweep 中断路由在单 running 不变量下继续成立),集成测试覆盖立即返回、estimating 阶段中断、终态 409、重启清扫、二次 queue 409、sweep 中断路由命中当前成员。Verify: `cargo test -p helixflow-server`
- [ ] `SP58-T4` 前端进行中响应、终态产物重取与 WS 对账。Owner: web 线程。Done when: queue/confirm 响应按进行中 run 渲染,进度由 WS 事件驱动;收到终态事件(`run.succeeded`/`run.failed`/`run.interrupted`)触发一次 `GET /api/workspaces/{id}/state` 全量快照重取并渲染 outputs(不新增产物专用事件);reducer 收到 run_id 不匹配事件时不丢弃、调度一次快照重取(与终态重取共用去重节流入口);WS 重连成功后重新拉取 workspace state 全量覆盖本地 run 状态,含前端测试。Verify: `cd web && npm test`

## 并行拆分

- 第一波并行:SP58-T1(store)、SP58-T2(run)、SP58-T4(web)三线并行,文件所有权不重叠。
- 第二波:SP58-T3 在 T1、T2 合入后串行执行(跨 crate 接线)。
- 文件所有权:
  - store 线程只改 `crates/store/src/run_records.rs`、`crates/store/src/lib.rs`(导出)。
  - run 线程只改 `crates/run/src/lib.rs`、`crates/run/src/cost_gate.rs`、`crates/run/src/error.rs`、`crates/run/src/tests/`。
  - server 线程只改 `crates/server/src/run_routes.rs`、`crates/server/src/sweep_support.rs`、`crates/server/src/app_state.rs`、`crates/server/src/main.rs`、`crates/server/src/run_routes_unavailable_tests.rs`。
  - web 线程只改 `web/src/api.ts`、`web/src/app.tsx`、`web/src/app.test.tsx` 及其引用的 workbench 状态组件;不改 `crates/`。
  - 任何线程不得修改其他线程所有权文件;共享类型变更(如 `RunOutcome` 语义)由 run 线程在 T2 一次性定型,server/web 只消费。

## 验证

- [ ] `SP58-T5` 全仓验证。Owner: 验证线程。依赖 SP58-T1 至 SP58-T4。Done when: 工作区级检查与测试全部通过且无未跟踪回归。Verify: `cargo check --workspace && cargo test --workspace && cd web && npm test && npm run build`
- [ ] `SP58-T6` 手工端到端验证。Owner: 验证线程。依赖 SP58-T5。Done when: 本地起服务跑慢节点 graph,确认请求即刻返回、WS 进度实时、run 完成后保持连接即看到产物、执行中重启后 run 为 `interrupted` 且 `waiting_confirmation` run 保留。Verify: `cargo run -p helixflow-server` + 浏览器 workbench 手工路径记录

## Handoff Notes

- 决策:spawn 在 run crate 内部(RunService 两阶段),不在 server 层;理由与竞态处理见 `specs/GH58/tech.md` 设计方案第 1 节。
- 决策:interrupt 条目在 run 记录创建同刻注册(先于 estimating),估算阶段 provider 调用与执行阶段统一纳入 `tokio::select!` 中断分支;终态写入统一走 `update_run_status_if_current` CAS,禁止无条件 `update_run_status` 覆盖终态。
- 决策:响应前事件竞态采取两层防御——服务端在持久化 queued 快照并构造完整响应对象后才 spawn;前端对 run_id 不匹配事件不丢弃、调度 state 快照重取。见 tech.md 设计方案 1/3。
- 决策:终态产物加载复用全量状态接口(终态 WS 事件触发一次 `GET /api/workspaces/{id}/state` 重取),不新增产物专用事件;重取入口统一并去重节流。
- 决策:sweep confirm 后成员保持 `queued`,轮到执行才置 `running`,组内至多一个 `running`,既有"取第一个 running"中断路由语义不变。
- 决策:并发策略维持"同 workspace 单活跃 run、跨 workspace 并发、无全局上限";不做分布式队列、不做任务重试(非目标)。
- 阻塞点:run crate 既有同步测试改造为"等待后台收敛"时,统一封装轮询/事件订阅辅助函数,避免 sleep 类时间断言。
- 证据:阻塞点与既有机制行号引用(run_routes.rs:52/:82/:141/:335,lib.rs:168/:199/:264/:417-420)已在 tech.md Codebase Context 固化,来自本 spec 起草会话的源码核读。
