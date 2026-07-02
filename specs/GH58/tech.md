# Tech Spec

## Linked Issue

GH-58

## Product Spec

`specs/GH58/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Run HTTP 路由 | `crates/server/src/run_routes.rs` | `queue_workspace_run`(:52)直接 `.await execute_manual_run` 执行到底;`confirm_run`(:82)同步执行;`interrupt_active_run`(:141)只接受 `queued`/`estimating`/`running`(:335);sweep 中断路由取组内第一个 `running` 的 run | 阻塞点所在;后台化后端点改为立即返回;sweep 中断路由语义依赖"组内至多一个 running" |
| Run 执行引擎 | `crates/run/src/lib.rs` | `execute_manual_run`(:208)创建 run 后同步执行;`RunInterrupt` 共享 map(:168,:199)只在执行期间存在条目;步间检查(:264)与 provider 调用 `tokio::select!`(:417-420)已支持中断 | 拆分"创建/认领"与"执行"两阶段,spawn 落点 |
| 估算阶段 | `crates/run/src/lib.rs`(`request_agent_run` / `request_sweep_plan`) | 创建 run 记录后置 `estimating`,同步 await provider 估算,期间 `RunInterrupt` map 无条目,此时中断通过状态检查后以 `RunNotActive` 失败 | estimating 阶段中断必须可靠命中(P3),需要提前注册 interrupt 条目并把估算调用纳入 `tokio::select!` |
| 确认流 | `crates/run/src/cost_gate.rs` | `confirm_run`(:75)认领(`waiting_confirmation`→`running`)后同步执行;`confirm_sweep_runs`(:204)顺序同步执行整组;`hold_run`(:80)将等待确认的 run 置为 `interrupted` | confirm 路径同样需要后台化 |
| Run 状态持久化 | `crates/store/src/run_records.rs` | 有 `update_run_status_if_current`(:232,CAS 语义)、`latest_workspace_run`(:189);无"列出所有活跃 run"查询 | 启动清扫需要新查询;CAS 用于终态竞争收敛 |
| 事件通道 | `crates/run/src/lib.rs`(`EventBus`,:102)、`crates/server/src/ws.rs`、`crates/store/src/run_records.rs`(`append_run_event`,:394) | 事件先落 `run_events` 表再 broadcast 到 `/ws`;WS 无历史回放 | 后台化后前端进度通道现成,无需新增协议 |
| 前端事件消费 | `web/src/api.ts`(`connectWorkspaceEvents`,:338)、`web/src/` workbench 组件 | WS 订阅 workspace 事件;断连仅置 offline 状态;reducer 丢弃 run_id 与当前 run 不匹配的事件;全量 state 当前偏向 latest/recommended run | 需要处理"立即返回的进行中响应"、终态产物加载、sweep 当前执行成员展示与重连对账;丢弃不匹配事件或 refetch 后仍返回 stale recommended run 都会造成状态丢失 |
| 服务启动 | `crates/server/src/main.rs`(`main`)、`crates/server/src/app_state.rs`(`AppState::open`) | 启动仅初始化 store 与路由,无 run 状态检查 | 僵尸 run 清扫的挂载点 |

## 设计方案

### 1. tokio::spawn 放在 run crate(RunService 内部)

spawn 放在 `crates/run`,不放 server 层。理由:中断句柄注册(`interrupts` map)、状态收敛、事件发射、实际成本记录(`record_actual_costs`)全部是 `RunService` 的内部职责;若 server 层 spawn,则"创建 run 记录后才能拿到 run_id"迫使 server 侵入 run crate 的两阶段内部结构。`RunService` 已满足 `Clone + Send + Sync + 'static`(泛型约束已存在),`self.clone()` move 进 `tokio::spawn` 即可。

执行模型拆为两阶段:

- 同步阶段(HTTP 请求内完成):compile plan、`create_run`(manual 路径)或 confirm 认领前置准备、创建 run_steps、**在 run 记录创建成功或 confirm run 可认领后、状态变成 `running` 之前注册 `RunInterrupt` 条目**(先于任何 estimating/执行动作)、持久化 queued/running 快照并构造完整的返回对象(`RunOutcome`),`tokio::spawn` 是同步阶段的最后一步。confirm 路径如果 CAS 认领失败,必须移除刚插入的 interrupt 条目并返回原错误,不能留下僵尸句柄。
- 后台阶段(spawn 的 task 内):`execute_created_run` 全流程 + `record_actual_costs`,结束后移除 interrupt 条目。后台任务的 `Err` 无法返回 HTTP,由既有逻辑落库 `failed` 状态并 emit `run.failed`;task 内再以 `tracing::error!` 记录(不允许静默吞错)。

**estimating 阶段中断覆盖**:`request_agent_run` / `request_sweep_plan` 在创建 run 记录时(即进入 `estimating` 之前)就把 `RunInterrupt` 条目插入 map,估算阶段的 provider 调用与执行阶段一样跑在 `tokio::select!` + 中断分支之下。estimating 期间收到中断时,估算调用被取消,run 经 CAS 收敛为 `interrupted`(不进入 `waiting_confirmation`),步骤全部置 `skipped`。由此 `RunInterrupt` 条目的生命周期统一为"run 记录创建 → run 终态",覆盖 `queued` / `estimating` / `running` 全部可中断状态,与 `interrupt_active_run` 的状态检查(:335)一致,消除 `RunNotActive` 竞态。

对外接口调整(不做向后兼容,直接替换):

- `RunService::start_manual_run(ManualRunRequest) -> RunResult<RunOutcome>`:返回创建时刻的 run(状态 `queued`)+ steps(全部 `queued`)+ 空 artifacts,执行已在后台。
- `RunService::start_confirmed_run(run_id) -> RunResult<RunOutcome>`:认领成功后返回(状态 `running`),执行在后台。
- `RunService::start_confirmed_sweep(run_ids, recommended_run_id) -> RunResult<Vec<RunOutcome>>`:整组成员 CAS 认领为 **`queued`**(`waiting_confirmation`→`queued`)后立即返回,组内顺序执行与推荐产物选择移入后台(复用现 `confirm_sweep_runs` 主体)。
- `interrupt_run` / `hold_run` 签名不变。

**sweep 中断路由(组内至多一个 running)**:sweep confirm 不把所有成员置 `running`。确认后全部成员保持 `queued`,后台任务按组内顺序执行,**轮到某成员执行时才把它 CAS `queued`→`running`**,该成员到达终态后再推进下一个。因此同一 sweep 组内任意时刻至多存在一个 `running` 的 run。server 路由不能只沿用"无 running 时回退请求 run"的现状;需要显式按组查找第一个 `running`,若不存在则查找第一个仍 `queued` 且有 interrupt 条目的成员作为目标。中断被路由到当前执行/首个 queued 成员后,后台任务在成员边界停止推进,组内尚未执行的 `queued` 成员统一收敛为 `interrupted`(步骤 `skipped`)。

关键竞态处理:

- interrupt 条目必须在 run 记录创建时插入 map 且先于 spawn;confirm 路径必须在状态从 `waiting_confirmation` 变成 `running` 之前插入 map,并在 CAS 失败或后续同步阶段失败时回滚移除。这样端点返回 run_id 后、estimating 期间以及 confirm 状态刚变成 `running` 的窗口内,任意中断请求都能命中;`queued` 阶段收到中断时,后台任务在首个步间检查(lib.rs:264 模式)即退出。
- **响应前事件丢失竞态(两层防御,服务端层)**:handler(经 RunService 同步阶段)先完成 queued 快照持久化并构造出完整的响应对象,之后才 `tokio::spawn` 后台任务。这保证响应体内容不依赖后台进度,且把"事件先于响应到达前端"的窗口压缩到网络传输段;窗口无法完全消除,剩余部分由前端防御(设计方案 3)兜底。
- 终态写入统一走 `update_run_status_if_current` CAS,中断与自然完成竞争时后写者失败,状态不被二次覆盖。

### 2. 端点立即返回 run_id + 初始状态

`queue_workspace_run` / `confirm_run` 复用现有 `RunConfirmationResponse` 形状:`run`(含 run_id、status、steps)、`outputs`(此时为空)、`pending_confirmation: None`。queue 返回 `status: "queued"`,confirm(单 run)返回 `status: "running"`,sweep confirm 返回的成员状态为 `queued`(见设计方案 1 sweep 段)。既有并发防护(`claim_workspace_run_queue` 请求内互斥 + `reject_active_workspace_run`)保持不变。

### 3. 前端从 WS 补齐进度与产物

前端已通过 `connectWorkspaceEvents` 订阅 `run.started` / `node.state` / `run.succeeded` / `run.failed` / `run.interrupted` 事件。调整:

- queue/confirm 响应到达后,前端把 run 视为进行中,进度由 WS 事件驱动更新。
- **终态产物加载**:前端收到终态事件(`run.succeeded` / `run.failed` / `run.interrupted`)时,触发一次 `GET /api/workspaces/{id}/state` 全量快照重取(复用既有对账机制),以快照中的 outputs / artifacts / sweep 推荐结果覆盖本地状态。不新增产物专用 WS 事件;终态事件只做状态信号,产物内容一律来自全量状态接口。保持连接的客户端由此在 run 完成时自动拿到产物,无需手动刷新或重连。sweep 后台任务必须在发出 UI 依赖的最终终态信号前先完成推荐 artifact selection 持久化;若现有成员终态事件早于 group finalization,实现必须额外在 group finalization 后触发一次现有事件类型或 state invalidation,但不能让唯一 refetch 信号早于 selection 写入。
- **sweep 当前成员对账**:`GET /api/workspaces/{id}/state` 不能只返回 latest/recommended run。后台 sweep 执行中,快照必须包含当前 `running` 成员或一个可渲染的 sweep aggregate/current member 字段,使早期成员的 `run.started` / `node.state` 事件触发 refetch 后能展示真实进度,而不是继续显示后创建的推荐 run 为 `queued`。
- **响应前事件丢失竞态(两层防御,前端层)**:reducer 收到 `run_id` 与当前 run 不匹配的事件时,不再丢弃,而是调度一次 `GET /api/workspaces/{id}/state` 全量快照重取(与终态重取共用同一去重/节流入口,避免事件风暴触发重复请求)。这样即使后台任务在响应体到达前就发出了 `run.started` 乃至终态事件,前端也会经快照收敛到服务端真实状态,不会停留在过期的 queued 快照上。
- WS `close`/`error` 后重连成功时,重新调用 `GET /api/workspaces/{id}/state`(已含 latest run + steps + outputs)做全量对账,以快照覆盖本地状态;不实现事件 seq 回放。

### 4. 重启僵尸 run 清扫

`crates/store` 新增 `interrupt_stale_active_runs()`:单次 SQL 事务把状态 IN (`queued`,`estimating`,`running`) 的 run 置为 `interrupted`,并把这些 run 下状态 IN (`queued`,`running`) 的 run_steps 置为 `skipped`,返回受影响 run 列表。`AppState::open`(server 启动路径)在路由服务前调用;对每个被清扫的 run 追加一条 `run.interrupted` 事件(data 标注 `{"reason":"server_restart"}`),保证事件流与状态一致。`waiting_confirmation` 不清扫:它没有执行进程,重启后用户仍可 confirm/hold。

### 5. 并发上限策略

维持"同一 workspace 至多 1 个活跃 run"(现 `reject_active_workspace_run` 语义),跨 workspace 允许并发,进程内不设全局上限。理由:单机本地工具,provider 调用是主要瓶颈且各 run 独立;引入全局队列/信号量属于分布式队列方向,是明确非目标。建议后续若出现资源争抢再以独立 issue 加全局并发闸。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1、P2(端点立即返回) | `crates/run/src/lib.rs`、`crates/server/src/run_routes.rs` | 集成测试:慢 mock provider 下 queue/confirm 返回时 run 非终态;`cargo test -p helixflow-server` |
| P3(排队/执行中中断) | `crates/run/src/lib.rs`(interrupt 建档时注册) | 单元测试:spawn 后立即 interrupt,收敛 `interrupted` + 全 `skipped`;`cargo test -p helixflow-run` |
| P3(estimating 阶段中断) | `crates/run/src/lib.rs`(`request_agent_run` / `request_sweep_plan` 估算调用纳入 `tokio::select!`) | 单元测试:慢 mock 估算 provider 挂起期间 interrupt,估算被取消、run 收敛 `interrupted` 且不进入 `waiting_confirmation`;`cargo test -p helixflow-run` |
| P3 边界(sweep 组内至多一个 running,中断路由命中当前成员) | `crates/run/src/cost_gate.rs`(sweep 后台顺序执行)、`crates/server/src/run_routes.rs`(既有路由) | 单元/集成测试:sweep confirm 后成员全为 `queued`;执行推进时任意时刻至多一个 `running`;执行中 interrupt 命中当前成员,未执行成员收敛 `interrupted` |
| P4、P5(waiting_confirmation/终态中断语义) | `crates/server/src/run_routes.rs`(既有行为保留) | 既有测试 `interrupt_route_rejects_waiting_confirmation_run` 继续通过 + 终态中断 409 测试 |
| P6(事件先落库再广播) | `crates/run/src/lib.rs` `emit`(既有) | 既有单元测试 + 后台路径事件断言 |
| P7(重连对账 + 终态产物快照重取) | `web/src/api.ts`、workbench 状态组件、state endpoint 的 sweep 当前成员投影 | `cd web && npm test`:模拟 WS 断连重连后以快照覆盖本地 run 状态;收到终态事件后发起一次 state 重取并渲染 outputs;后台 sweep 早期成员事件触发 refetch 后展示当前 running 成员 |
| P7 边界(响应前事件竞态两层防御) | `crates/run/src/lib.rs`(同步阶段持久化+构造响应后才 spawn)、`web/src/` reducer | run crate 单测:断言 spawn 前 queued 快照已落库且 `RunOutcome` 已构造;前端测试:注入 run_id 不匹配事件,断言不丢弃并触发一次 state 快照重取 |
| P8(sweep selection 先于 UI 终态 refetch 可见) | `crates/run/src/cost_gate.rs`、workspace state 输出 | 集成测试:sweep 最后成员完成时先持久化推荐 selection,再触发 UI 依赖的终态/refetch 信号;保持连接客户端 refetch 后能看到推荐产物 |
| P9(重启清扫) | `crates/store/src/run_records.rs`、`crates/server/src/app_state.rs` | 单元测试:预置 `running`/`waiting_confirmation` 记录,启动后前者 `interrupted`、后者不变 |
| P10(同 workspace 单活跃 run) | `crates/server/src/run_routes.rs`(既有) | 集成测试:后台 run 进行中再次 queue 返回 409 |
| P11(后台失败不静默) | `crates/run/src/lib.rs` spawn 包装 | 单元测试:provider 失败时 run 收敛 `failed` 且存在 `run.failed` 事件 |

## 数据流

- 输入:HTTP `POST /api/workspaces/{id}/runs/queue`、`POST .../runs/{run_id}/confirm|hold`、`POST /api/runs/{run_id}/interrupt`。
- 同步路径:compile plan → 写 `runs`/`run_steps`(SQLite,经 `Store`)→ 注册 interrupt(建档同刻;confirm 路径先注册再 CAS 到 `running`,失败则回滚移除)→ 构造完整响应对象 → spawn → 返回 `RunConfirmationResponse`。
- 后台路径:逐步执行(sweep 组内成员轮到时才置 `running`)→ provider 调用(估算与执行均在 `tokio::select!` 中断分支下)→ 写 `run_steps`/`artifacts`/`cost_ledger` → sweep 推荐 selection 持久化早于 UI 依赖的最终 refetch 信号 → 每次迁移 `append_run_event`(`run_events` 表)→ `EventBus` broadcast → `/ws` 推送前端。
- 启动路径:`AppState::open` → `interrupt_stale_active_runs()` 事务更新 `runs`/`run_steps` → 追加 `run.interrupted` 事件。
- 前端:响应 run_id → WS 事件增量更新 → 终态事件或 run_id 不匹配事件触发 `GET /api/workspaces/{id}/state` 全量快照重取 → 后台 sweep 事件触发的快照显示当前 running 成员/aggregate → 重连时同接口全量对账。

## 备选方案

- server 层 spawn(run crate 不动):被否。需要 server 感知 run crate 的创建/执行拆分细节,interrupt 注册窗口难以保证,职责泄漏。
- 持久化任务队列表 + 独立 worker 轮询:被否。单机场景过度设计,属于明确非目标(分布式队列方向)。
- WS 事件 seq 回放(重连补发缺失事件):被否。全量状态接口已可对账,回放协议增加复杂度收益有限。
- 终态时通过 WS 推送产物专用事件(artifact payload):被否。产物体积不可控,广播缓冲(128)易溢出;全量状态接口已含 outputs,终态事件做信号 + 快照重取即可,单一事实来源。
- sweep confirm 时把全组置 `running`:被否。使"取第一个 running"的中断路由无法区分当前执行成员与未执行成员,需要额外 current-run 标记;改为成员保持 `queued`、轮到执行才置 `running`,并把无 running 时的 route fallback 改为组内第一个 queued 成员。
- 启动清扫标记为 `failed` 而非 `interrupted`:被否。重启不是 run 自身错误,issue 明确要求 `interrupted`。

## 风险

- Security: 无新增外部输入面;中断端点权限模型不变。
- Compatibility: queue/confirm 响应从"含最终结果"变为"含进行中快照",前端同仓同步改造,无外部消费者。
- Performance: 后台任务与请求线程解耦,WS broadcast 缓冲(默认 128)慢消费者丢广播——事实来源在 `run_events` 表,可接受;终态/不匹配事件触发的快照重取需去重节流,避免同一终态触发多次全量请求。
- Maintenance: 两阶段拆分后 `execute_manual_run` 旧同步入口删除,run crate 既有测试需改为等待后台收敛(轮询 store 或订阅事件),测试写法需统一辅助函数避免时间断言脆弱。

## 测试计划

- [ ] Unit tests: run crate 两阶段拆分(queued 阶段中断、estimating 阶段中断——慢 mock 估算 provider 下估算调用被 `tokio::select!` 取消、执行中中断、confirm 路径先注册 interrupt 再 CAS `running` 且失败回滚、后台失败收敛、interrupt 条目"建档即注册、终态即移除"生命周期、spawn 前 queued 快照已持久化且响应对象已构造);sweep 后台顺序执行(confirm 后成员全 `queued`、任意时刻至多一个 `running`、中断命中当前或首个 queued 成员且未执行成员收敛 `interrupted`、推荐 selection 先于最终 UI refetch 信号持久化);store 清扫方法(活跃 run 转 `interrupted`、步骤转 `skipped`、`waiting_confirmation` 不动)。
- [ ] Integration tests: server 路由层——queue/confirm 立即返回、estimating 阶段 interrupt 经路由收敛 `interrupted`、confirm 刚置 `running` 的竞态窗口 interrupt 可命中、终态中断 409、重启清扫(重开 AppState)、同 workspace 二次 queue 409、sweep confirm 后台化 + 中断路由命中当前 `running` 或首个 `queued` 成员、workspace state refetch 能展示当前 sweep running 成员且最终推荐产物已持久化。
- [ ] Frontend tests: `cd web && npm test`——终态事件(`run.succeeded`/`run.failed`/`run.interrupted`)触发一次 state 快照重取并渲染 outputs;run_id 不匹配事件不丢弃、调度快照重取;后台 sweep 早期成员事件触发 refetch 后展示当前 running 成员/aggregate,最终 refetch 展示推荐产物;WS 断连重连后全量对账覆盖本地状态;重取请求去重(同一终态不重复拉取)。
- [ ] Manual verification: 本地起服务,跑含 mock 延迟节点的 graph,验证请求即刻返回、前端进度实时更新、run 完成后保持连接即看到产物、执行中刷新页面/断 WS 后状态一致、执行中重启服务后 run 显示 `interrupted`。

## 回滚方案

单 PR 纯代码变更,无 schema 迁移:`git revert` 该 PR 即回到同步执行模型。启动清扫写入的 `interrupted` 状态是合法终态,回滚后不需要数据修复。
