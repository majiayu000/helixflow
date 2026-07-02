# Tech Spec

## Linked Issue

GH-58

## Product Spec

`specs/GH58/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Run HTTP 路由 | `crates/server/src/run_routes.rs` | `queue_workspace_run`(:52)直接 `.await execute_manual_run` 执行到底;`confirm_run`(:82)同步执行;`interrupt_active_run`(:141)只接受 `queued`/`estimating`/`running`(:335) | 阻塞点所在;后台化后端点改为立即返回 |
| Run 执行引擎 | `crates/run/src/lib.rs` | `execute_manual_run`(:208)创建 run 后同步执行;`RunInterrupt` 共享 map(:168,:199)只在执行期间存在条目;步间检查(:264)与 provider 调用 `tokio::select!`(:417-420)已支持中断 | 拆分"创建/认领"与"执行"两阶段,spawn 落点 |
| 确认流 | `crates/run/src/cost_gate.rs` | `confirm_run`(:75)认领(`waiting_confirmation`→`running`)后同步执行;`confirm_sweep_runs`(:204)顺序同步执行整组;`hold_run`(:80)将等待确认的 run 置为 `interrupted` | confirm 路径同样需要后台化 |
| Run 状态持久化 | `crates/store/src/run_records.rs` | 有 `update_run_status_if_current`(:232,CAS 语义)、`latest_workspace_run`(:189);无"列出所有活跃 run"查询 | 启动清扫需要新查询;CAS 用于终态竞争收敛 |
| 事件通道 | `crates/run/src/lib.rs`(`EventBus`,:102)、`crates/server/src/ws.rs`、`crates/store/src/run_records.rs`(`append_run_event`,:394) | 事件先落 `run_events` 表再 broadcast 到 `/ws`;WS 无历史回放 | 后台化后前端进度通道现成,无需新增协议 |
| 前端事件消费 | `web/src/api.ts`(`connectWorkspaceEvents`,:338)、`web/src/` workbench 组件 | WS 订阅 workspace 事件;断连仅置 offline 状态 | 需要处理"立即返回的进行中响应"与重连对账 |
| 服务启动 | `crates/server/src/main.rs`(`main`)、`crates/server/src/app_state.rs`(`AppState::open`) | 启动仅初始化 store 与路由,无 run 状态检查 | 僵尸 run 清扫的挂载点 |

## 设计方案

### 1. tokio::spawn 放在 run crate(RunService 内部)

spawn 放在 `crates/run`,不放 server 层。理由:中断句柄注册(`interrupts` map)、状态收敛、事件发射、实际成本记录(`record_actual_costs`)全部是 `RunService` 的内部职责;若 server 层 spawn,则"创建 run 记录后才能拿到 run_id"迫使 server 侵入 run crate 的两阶段内部结构。`RunService` 已满足 `Clone + Send + Sync + 'static`(泛型约束已存在),`self.clone()` move 进 `tokio::spawn` 即可。

执行模型拆为两阶段:

- 同步阶段(HTTP 请求内完成):compile plan、`create_run`(manual 路径)或 CAS 认领 `waiting_confirmation`→`running`(confirm 路径)、创建 run_steps、**注册 `RunInterrupt` 条目**、`tokio::spawn` 后台任务。
- 后台阶段(spawn 的 task 内):`execute_created_run` 全流程 + `record_actual_costs`,结束后移除 interrupt 条目。后台任务的 `Err` 无法返回 HTTP,由既有逻辑落库 `failed` 状态并 emit `run.failed`;task 内再以 `tracing::error!` 记录(不允许静默吞错)。

对外接口调整(不做向后兼容,直接替换):

- `RunService::start_manual_run(ManualRunRequest) -> RunResult<RunOutcome>`:返回创建时刻的 run(状态 `queued`)+ steps(全部 `queued`)+ 空 artifacts,执行已在后台。
- `RunService::start_confirmed_run(run_id) -> RunResult<RunOutcome>`:认领成功后返回(状态 `running`),执行在后台。
- `RunService::start_confirmed_sweep(run_ids, recommended_run_id) -> RunResult<Vec<RunOutcome>>`:整组 CAS 认领成功后立即返回,组内顺序执行与推荐产物选择移入后台(复用现 `confirm_sweep_runs` 主体)。
- `interrupt_run` / `hold_run` 签名不变。

关键竞态处理:interrupt 条目必须在 spawn 之前插入 map,保证端点返回 run_id 后任意时刻的中断请求都能命中;`queued` 阶段收到中断时,后台任务在首个步间检查(lib.rs:264 模式)即退出。终态写入统一走 `update_run_status_if_current` CAS,中断与自然完成竞争时后写者失败,状态不被二次覆盖。

### 2. 端点立即返回 run_id + 初始状态

`queue_workspace_run` / `confirm_run` 复用现有 `RunConfirmationResponse` 形状:`run`(含 run_id、status、steps)、`outputs`(此时为空)、`pending_confirmation: None`。queue 返回 `status: "queued"`,confirm 返回 `status: "running"`。既有并发防护(`claim_workspace_run_queue` 请求内互斥 + `reject_active_workspace_run`)保持不变。

### 3. 前端从 WS 补齐进度

前端已通过 `connectWorkspaceEvents` 订阅 `run.started` / `node.state` / `run.succeeded` / `run.failed` / `run.interrupted` 事件。调整:

- queue/confirm 响应到达后,前端把 run 视为进行中,进度与最终结果完全由 WS 事件驱动更新。
- WS `close`/`error` 后重连成功时,重新调用 `GET /api/workspaces/{id}/state`(已含 latest run + steps + outputs)做全量对账,以快照覆盖本地状态;不实现事件 seq 回放。

### 4. 重启僵尸 run 清扫

`crates/store` 新增 `interrupt_stale_active_runs()`:单次 SQL 事务把状态 IN (`queued`,`estimating`,`running`) 的 run 置为 `interrupted`,并把这些 run 下状态 IN (`queued`,`running`) 的 run_steps 置为 `skipped`,返回受影响 run 列表。`AppState::open`(server 启动路径)在路由服务前调用;对每个被清扫的 run 追加一条 `run.interrupted` 事件(data 标注 `{"reason":"server_restart"}`),保证事件流与状态一致。`waiting_confirmation` 不清扫:它没有执行进程,重启后用户仍可 confirm/hold。

### 5. 并发上限策略

维持"同一 workspace 至多 1 个活跃 run"(现 `reject_active_workspace_run` 语义),跨 workspace 允许并发,进程内不设全局上限。理由:单机本地工具,provider 调用是主要瓶颈且各 run 独立;引入全局队列/信号量属于分布式队列方向,是明确非目标。建议后续若出现资源争抢再以独立 issue 加全局并发闸。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1、P2(端点立即返回) | `crates/run/src/lib.rs`、`crates/server/src/run_routes.rs` | 集成测试:慢 mock provider 下 queue/confirm 返回时 run 非终态;`cargo test -p helixflow-server` |
| P3(排队/执行中中断) | `crates/run/src/lib.rs`(interrupt 提前注册) | 单元测试:spawn 后立即 interrupt,收敛 `interrupted` + 全 `skipped`;`cargo test -p helixflow-run` |
| P4、P5(waiting_confirmation/终态中断语义) | `crates/server/src/run_routes.rs`(既有行为保留) | 既有测试 `interrupt_route_rejects_waiting_confirmation_run` 继续通过 + 终态中断 409 测试 |
| P6(事件先落库再广播) | `crates/run/src/lib.rs` `emit`(既有) | 既有单元测试 + 后台路径事件断言 |
| P7(重连对账) | `web/src/api.ts`、workbench 状态组件 | `cd web && npm test`:模拟 WS 断连重连后以快照覆盖本地 run 状态 |
| P8(重启清扫) | `crates/store/src/run_records.rs`、`crates/server/src/app_state.rs` | 单元测试:预置 `running`/`waiting_confirmation` 记录,启动后前者 `interrupted`、后者不变 |
| P9(同 workspace 单活跃 run) | `crates/server/src/run_routes.rs`(既有) | 集成测试:后台 run 进行中再次 queue 返回 409 |
| P10(后台失败不静默) | `crates/run/src/lib.rs` spawn 包装 | 单元测试:provider 失败时 run 收敛 `failed` 且存在 `run.failed` 事件 |

## 数据流

- 输入:HTTP `POST /api/workspaces/{id}/runs/queue`、`POST .../runs/{run_id}/confirm|hold`、`POST /api/runs/{run_id}/interrupt`。
- 同步路径:compile plan → 写 `runs`/`run_steps`(SQLite,经 `Store`)→ 注册 interrupt → spawn → 返回 `RunConfirmationResponse`。
- 后台路径:逐步执行 → provider 调用(`tokio::select!` 中断)→ 写 `run_steps`/`artifacts`/`cost_ledger` → 每次迁移 `append_run_event`(`run_events` 表)→ `EventBus` broadcast → `/ws` 推送前端。
- 启动路径:`AppState::open` → `interrupt_stale_active_runs()` 事务更新 `runs`/`run_steps` → 追加 `run.interrupted` 事件。
- 前端:响应 run_id → WS 事件增量更新 → 重连时 `GET /api/workspaces/{id}/state` 全量对账。

## 备选方案

- server 层 spawn(run crate 不动):被否。需要 server 感知 run crate 的创建/执行拆分细节,interrupt 注册窗口难以保证,职责泄漏。
- 持久化任务队列表 + 独立 worker 轮询:被否。单机场景过度设计,属于明确非目标(分布式队列方向)。
- WS 事件 seq 回放(重连补发缺失事件):被否。全量状态接口已可对账,回放协议增加复杂度收益有限。
- 启动清扫标记为 `failed` 而非 `interrupted`:被否。重启不是 run 自身错误,issue 明确要求 `interrupted`。

## 风险

- Security: 无新增外部输入面;中断端点权限模型不变。
- Compatibility: queue/confirm 响应从"含最终结果"变为"含进行中快照",前端同仓同步改造,无外部消费者。
- Performance: 后台任务与请求线程解耦,WS broadcast 缓冲(默认 128)慢消费者丢广播——事实来源在 `run_events` 表,可接受。
- Maintenance: 两阶段拆分后 `execute_manual_run` 旧同步入口删除,run crate 既有测试需改为等待后台收敛(轮询 store 或订阅事件),测试写法需统一辅助函数避免时间断言脆弱。

## 测试计划

- [ ] Unit tests: run crate 两阶段拆分(queued 阶段中断、执行中中断、后台失败收敛、interrupt 条目生命周期);store 清扫方法(活跃 run 转 `interrupted`、步骤转 `skipped`、`waiting_confirmation` 不动)。
- [ ] Integration tests: server 路由层——queue/confirm 立即返回、终态中断 409、重启清扫(重开 AppState)、同 workspace 二次 queue 409、sweep confirm 后台化。
- [ ] Manual verification: 本地起服务,跑含 mock 延迟节点的 graph,验证请求即刻返回、前端进度实时更新、执行中刷新页面/断 WS 后状态一致、执行中重启服务后 run 显示 `interrupted`。

## 回滚方案

单 PR 纯代码变更,无 schema 迁移:`git revert` 该 PR 即回到同步执行模型。启动清扫写入的 `interrupted` 状态是合法终态,回滚后不需要数据修复。
