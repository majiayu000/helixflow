# Product Spec

## Linked Issue

GH-58

## 用户问题

用户在 workbench 触发 run(queue 或 confirm)后,HTTP 请求会同步阻塞直到整个 run 执行完毕。视频生成类长任务需要数分钟,期间请求连接一直挂起,前端无法及时拿到 run_id,也无法感知进度;中断只能依赖并发的第二个请求在极窄的时间窗口内置位标志,可靠性差。此外,如果服务器在 run 执行期间重启,数据库中会残留 `running` 状态的僵尸记录,前端会永远认为该 workspace 有活跃 run,导致后续无法再触发新 run。

## 目标

- queue/confirm 端点立即返回 run_id 与初始状态,不等待执行完成。
- run 执行移入后台任务,进度通过既有 `/ws` 事件流推送给前端。
- 中断在 run 的任意执行阶段(排队、执行中)可靠生效,run 与 run_steps 状态一致收敛。
- 服务器启动时清扫僵尸 run:将残留的活跃状态 run 标记为 `interrupted`。

## 非目标

- 不做分布式任务队列,不引入外部消息中间件。
- 不做持久化任务重试(失败/中断的 run 不自动重跑)。
- 不做并行节点执行、节点结果缓存(独立 issue)。
- 不改变同一 workspace 至多一个活跃 run 的既有约束。
- 不新增 WS 事件回放协议;断连恢复靠既有全量状态接口对账。

## Behavior Invariants

1. 用户触发 queue run 后,HTTP 响应必须在执行开始前返回,响应体包含 run_id 与该 run 的当前状态;不因图中节点耗时而延迟。
2. 用户触发 confirm run 后,HTTP 响应立即返回,run 状态为 `running`(已被认领),实际步骤执行在后台进行。
3. run 处于 `queued` / `estimating` / `running` 任一状态时,调用中断端点必须成功;之后该 run 最终收敛到 `interrupted`,未执行的步骤全部为 `skipped`,不产生新的步骤副作用。
4. run 处于 `waiting_confirmation` 状态时,中断端点保持既有行为返回冲突;取消该 run 走 hold 端点,结果为 `interrupted`。
5. run 已到达终态(`succeeded` / `failed` / `interrupted`)后,中断请求返回冲突错误,run 状态不变。
6. 后台执行的每一次状态迁移(run.started、node.state、run.succeeded、run.failed、run.interrupted)都必须先写入 run_events 再广播到 `/ws`,事件 seq 单调递增。
7. 前端与服务端的最终一致靠全量状态接口保证:WS 断连重连后重新拉取 workspace 全量状态即与服务端一致;保持连接的前端在收到终态事件(run.succeeded / run.failed / run.interrupted)后,通过一次全量状态重取拿到 outputs 等产物,无需手动刷新或重连;不会出现"事件丢失或事件先于 HTTP 响应到达,导致 run 永远显示执行中/产物缺失"的状态。
8. sweep 后台执行期间,全量状态接口必须能表达组内当前执行成员或聚合进度,不能只返回最新/推荐 run 导致早期成员事件触发的 refetch 仍显示 stale queued;当 sweep 到达最终推荐结果时,推荐产物/selection 必须先持久化,之后才发出 UI 依赖的最终终态信号或可被终态 refetch 观察到。
9. 服务器重启后,启动完成时数据库中不存在 `queued` / `estimating` / `running` 状态的 run;这些 run 被标记为 `interrupted`,其未终态步骤被标记为 `skipped`。`waiting_confirmation` 的 run 不受重启清扫影响,仍可被用户确认或取消。
10. 同一 workspace 已有活跃 run(`queued` / `estimating` / `waiting_confirmation` / `running`)时,再次 queue 返回冲突错误;不同 workspace 的 run 可以并发执行。
11. 后台任务失败时,run 状态收敛为 `failed` 且错误信息写入 run 记录与 run.failed 事件;不允许静默丢失失败结果。

## 验收标准

- [ ] 长任务(mock provider 人为延迟)执行期间,queue/confirm 请求在执行完成前返回,响应含 run_id。
- [ ] 在 `queued` / `estimating` / `running` 任意阶段调用 interrupt,run 收敛为 `interrupted`,剩余步骤为 `skipped`。
- [ ] `waiting_confirmation` 的 run 经 hold 收敛为 `interrupted`,经 interrupt 端点返回冲突。
- [ ] 服务器重启后,先前 `running` 的 run 变为 `interrupted`,`waiting_confirmation` 的 run 保留原状态。
- [ ] 前端断开并重连 WS 后,run 状态展示与服务端一致(通过全量状态接口对账)。
- [ ] 保持 WS 连接的前端在 run 到达终态后,无需手动刷新即可看到 outputs 等产物(终态事件触发全量状态重取)。
- [ ] sweep confirm 后,当前执行成员进度能通过全量状态接口展示;最终推荐产物在终态 refetch 时已经可见。
- [ ] 同 workspace 存在活跃后台 run 时,再次 queue 返回 409。

## 边界情况

- 端点返回 run_id 与后台任务真正开始之间存在窗口:中断句柄必须在 run 记录创建时(进入 `estimating` 之前)注册,保证此窗口内以及估算期间的中断请求都可命中。
- 后台任务可能在前端处理完 HTTP 响应之前就发出事件:前端收到 run_id 与当前 run 不匹配的事件时,不丢弃,而是触发一次全量状态重取,收敛到服务端真实状态。
- 后台任务执行中进程被强杀:无法收敛状态,由下次启动清扫兜底(标记 `interrupted`)。
- sweep 组 confirm:多个 run 顺序执行,确认后成员保持 `queued` 并立即返回,轮到某成员执行时才置为 `running`,同组内任意时刻至多一个 `running`;中断路由到当前执行成员后,组内未执行成员收敛为 `interrupted`;若组内尚无 `running`,中断必须路由到组内第一个仍 queued 的成员而不是回退到请求里的任意 run。
- sweep 组产物选择:最终推荐结果写入必须发生在 UI 会据以 refetch 的终态信号之前,避免终态事件触发的全量状态重取读到尚未持久化的推荐产物。
- 中断请求与 run 自然完成竞争:以先落库的终态为准,后到者返回冲突,不产生二次状态覆盖。
- WS 广播缓冲溢出(慢消费者):允许丢广播,run_events 表为事实来源,前端对账接口兜底。

## 发布说明

- API 行为变更:queue/confirm 响应不再包含最终执行结果(outputs 在执行完成前为空、状态为进行中),前端改为依赖 WS 事件与全量状态接口获取结果。本项目前后端同仓同发,无需兼容旧响应语义。
- 无数据库 schema 迁移;仅新增启动时对既有 run 记录的状态清扫写入。
