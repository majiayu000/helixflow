# Product Spec

## Linked Issue

GH-154

## 用户问题

服务重启时，`crates/server/src/app_state.rs` 会调用
`Store::interrupt_stale_active_runs`，把 `queued`、`estimating`、`running` run
及其未完成 step 直接改为 `interrupted`/`skipped`。与此同时，Atlas 与 fal 的远端
task handle 只保存在 gateway 进程内的 `in_flight` map；进程退出后，本地既无法继续
轮询，也无法补取消或回收远端结果。远端任务可能继续执行并计费，但用户只能看到本地
run 已中断。

## 目标

- 在发起远端请求前持久化 dispatch intent，在 provider 返回 handle 后原子激活该
  handle；SQLite 是恢复、轮询与取消的唯一事实源。
- 服务启动时短事务扫描并租约认领可恢复 run，然后在后台恢复远端轮询和剩余 DAG，
  不阻塞服务启动。
- 恢复结果幂等地写入 artifact、step output、cost ledger、step/run 终态和事件；任意
  次重启都不能产生重复输出或重复费用记录。
- 无法恢复时优先尝试 provider 支持的补取消；仍无法确认远端终态时显式中断并写入
  持久化计费风险事件。
- `queued` run 默认保持当前向后兼容行为（重启后中断）；只有严格有效且显式开启的
  配置才允许 requeue。

## 非目标

- 不给 Atlas 发明不存在的 cancel endpoint；保持 #124 的能力边界。
- 不引入跨进程分布式队列、多实例调度或 leader election；lease 只封闭重入与进程
  重启窗口。
- 不改变在线手动 interrupt 的产品语义、cost confirmation 或 workspace active-run
  约束。
- 不在本 issue 实现 #153 的 Agent 自动修图；本 issue 只提供统一的 recovered-failed
  终态入口。
- 不自动重放没有持久化 handle 且无法证明 provider 幂等的远端提交。

## Behavior Invariants

1. `run_provider_tasks` 是远端 task handle 的唯一 durable truth；`active_handles`
   内存 map 不再决定恢复或取消结果。
2. 每次 provider 提交前必须先完成不会发网络请求的本地 preflight，再写入唯一
   `dispatching` intent；提交完成后只能通过 compare-and-swap 将同一记录推进为
   `active`。dispatch 失败必须区分 `not_submitted`、`rejected` 和 `outcome_unknown`：
   前两者进入共同 failed finalizer，只有结果不确定时进入 `abandoned` 计费风险。
3. 进程若在远端接受请求后、handle CAS 前崩溃，数据库只会留下 `dispatching`。
   Atlas/fal 当前没有可依赖的 submit idempotency 保证，因此启动恢复不得自动重发；
   该记录进入 `abandoned` 并产生计费风险。
4. 持久化 handle 只能包含 provider identity、task identity、dispatch 时的非秘密
   `recovery_scope_fingerprint` 和经过校验的轮询/结果定位信息；不得包含 API key、
   Authorization header、cookie、userinfo、fragment 或 secret-bearing query。恢复或
   取消前，当前 provider scope 必须与指纹完全一致，配置/account 漂移时 fail closed。
5. 持久化 URL 在首次写入和每次恢复请求前都必须按当前 provider 配置重新校验
   scheme、origin 与允许路径；URL 不通过时 fail closed，不发网络请求。
6. 日志、事件、artifact metadata、API 和前端不得暴露 task identity、完整远端 URL、
   credential、auth header 或原始 provider 响应；用户可见数据只包含 provider、稳定
   reason code 与脱敏说明。
7. 启动扫描只做分类、CAS/lease claim 和 durable 终态写入；实际 poll、artifact
   下载、补取消和 DAG continuation 必须在 `AppState` 使用的同一个 `Store`/pool
   上由后台任务执行。
8. recovery lease 有唯一 owner、有限期限和续租；持有有效 lease 的 worker 才能改变
   该 run 的恢复状态。lease 丢失后 worker 必须停止写入，过期 lease 可被后续进程
   重新认领。
9. `running` run 的 `active` handle 可恢复轮询；远端成功时产物、输出端口、费用与
   step 终态完整持久化，并从 durable step outputs 继续剩余 DAG。
10. 已 `succeeded` step 不重新执行；正常执行、builtin 和 cache hit 都必须通过同一个
    output/step finalizer 写 `run_step_outputs`。恢复重建输入只读取
    `run_step_outputs` → `artifacts`，不得根据文件名、最新 artifact 或内存 map 猜测。
11. 同一 step/port 至多有一个 durable output，同一费用 operation key 至多有一条
    ledger 记录；恢复重放返回既有记录，不复制 artifact row 或费用。
12. provider 返回远端 failed 时，step/run 进入 `failed`，保留脱敏错误和事件，再进入
    与正常执行相同的 failure finalizer；不得转成 `interrupted` 或静默跳过。
13. provider 暂时不可轮询时按有界退避重试并续租；超过恢复期限后，若 provider 支持
    cancel 则补取消。取消成功记录 `cancelled` 并中断 run；取消失败或不支持则记录
    `abandoned` 与计费风险。
14. Atlas 支持从持久化 prediction identity 恢复 poll，但仍不支持 cancel；不得构造
    虚假的 Atlas cancel URL。fal 使用经 revalidation 的 status/result 定位恢复，并
    可按现有语义执行补取消。
15. mock 只用于明确启用的本地/测试环境；测试恢复 handle 可由 frozen request
    确定性重建结果，不能把 mock output 伪装为真实 provider output。
16. 缺少 handle 的 provider-backed `running` step、遗留 active row、非法 handle 或
    `dispatching` crash window 都是显式不可恢复状态；run 进入 `interrupted`，task
    进入 `abandoned`，并原子持久化与 `run.remote_cancel_unsupported` 同级的计费风险
    event 和 system message；刷新或出现后续 run 后风险仍可见。
17. `queued` run 在 `HELIXFLOW_RUN_REQUEUE_ON_RESTART` 缺失或为 false 时保持现状：
    进入 `interrupted`；只有显式 true、frozen plan 合法且没有 provider task 时才可
    requeue。同名配置非法值必须阻止启动，不得静默 fallback。
18. `estimating` 默认中断；`waiting_confirmation` 不参与启动清理，不能绕过用户成本
    确认。requeue 不得创建新 run、改变 estimate 或重复写 estimate ledger。
19. terminal transition、对应 run event 和 task/step final state 必须由一个 store
    transaction/CAS 决定；重复恢复不会重复终态事件，也不会把 terminal run 改回
    active。
20. workspace 单 active-run 规则和 sweep `group_id` 例外保持不变；恢复不能让同一
    workspace 的互斥 run 同时继续执行。
21. 在线 interrupt 改为从数据库读取 active handles，并在一个事务中以
    `running → interrupted` CAS 撤销 recovery lease。recovery finalizer 必须同时校验
    run 仍为 running 且 lease/owner 有效。非最终 step 的 poll-complete 若先提交，其
    artifact/cost/output 保留，run 仍为 running，随后 interrupt 可中断剩余 DAG；若
    interrupt 先提交，未提交的 step finalizer 必须停止。只有“最后一个 step + run
    terminal”与 interrupt 竞争时才是单一 run-terminal CAS 胜者。进程内 cancellation
    token 只负责加速停止，不作为正确性真相；用户可见成功、失败、unsupported 和计费
    风险语义保持 #124 既有契约。
22. #154 必须先于 #153 实现；恢复得到的 `failed` run 调用共同 failure finalizer，
    以便 #153 后续在同一入口接 Agent 修图，且不会产生第二套重试/修图循环。

## 验收标准

- [ ] 远端 dispatch intent 与 handle 持久化，关闭并重新打开同一数据库后仍可查询；
      运行期取消不依赖 gateway 内存 map。
- [ ] 模拟 dispatch 前、远端接受后/handle CAS 前、handle active 后、结果写入中和
      terminal commit 后五个 crash point；每个点的恢复结果确定且无重复提交。
- [ ] dispatch 的本地未提交、上游明确拒绝与结果未知三类错误有确定性测试；只有结果
      未知产生 abandoned/计费风险，前两类进入 failed finalizer 且不自动重提。
- [ ] `running + active handle` 可在重启后恢复到 succeeded/failed；成功路径产物、
      `run_step_outputs`、cost ledger 和剩余 DAG 完整。
- [ ] fal 无法继续恢复时执行补取消；Atlas 不支持 cancel 时进入 abandoned 并产生
      durable billing-risk event，且事件/日志无 task id、完整 URL 或 secret。
- [ ] queued 默认中断；只有显式 true 才 requeue；非法配置阻止启动；
      `waiting_confirmation` 永不自动启动。
- [ ] lease 竞争、续租、过期认领和 lease loss 均有 store/run 集成测试；两个 worker
      不能同时 finalize 同一 run。
- [ ] mock、fal、Atlas 覆盖恢复、补取消和不可恢复路径；provider URL origin 变化、
      account/scope 变化、userinfo/query secret、恶意 URL 均在网络请求前拒绝。
- [ ] cache hit 后、下游启动前崩溃可从 durable output 继续；poll-complete、在线
      interrupt、cancel 三方竞态只能提交一个终态。
- [ ] artifact hydration/API 序列化测试证明 task id、poll/result URL 与 provider 原始
      payload 不会离开 store/gateway 边界。
- [ ] Web 在实时事件和断线补拉后都显示恢复、补取消和计费风险，不伪装为成功；刷新
      后 durable 风险仍可见。
- [ ] 正常执行、在线 interrupt、cost confirmation、sweep 与同图 self-heal 回归通过；
      recovered failed 进入共同 failure finalizer，但不实现 #153 Agent 修图。

## 发布说明

首个版本保持 `HELIXFLOW_RUN_REQUEUE_ON_RESTART=false`，只启用 durable handle 与
running recovery。上线前用 crash-point 测试验证 Atlas/fal 的恢复窗口；上线后观察
`run.recovery_started`、`run.recovery_succeeded`、`run.recovery_cancelled`、
`run.recovery_abandoned` 的数量和原因。回滚恢复 worker 时保留新增表和写入路径，
关闭后台恢复只允许将 active task 显式转为 interrupted/abandoned，不能恢复旧的
静默批量清理。
