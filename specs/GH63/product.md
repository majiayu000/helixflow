# Product Spec

## Linked Issue

GH-63

## 用户问题

执行计划现在是拓扑排序后的线性列表,无依赖分支也严格串行。用户在同一 prompt 下并发生成多张候选图时,实际可以并行执行,但当前耗时接近所有分支耗时之和。

## 目标

- run executor 按依赖就绪度调度 step,无依赖或依赖已完成的分支可并发执行。
- 并发度可配置,默认保守,避免压垮本地资源或 provider。
- run_events seq、step 状态和 artifacts 在并发下保持一致。
- 中断能取消所有 in-flight step,未开始 step 收敛为 skipped/interrupted。

## 非目标

- 不做跨 run 调度、多机执行或分布式队列。
- 不改变 graph 语义或 provider 节点定义。
- 不在本 issue 实现节点缓存,只消费 GH-62 结果。

## Behavior Invariants

1. 菱形图 A -> B、A -> C、B/C -> D 中,B 和 C 在 A 成功后可并发执行,D 必须等待 B/C 都成功。
2. 同一 run 的并发 step 数不得超过配置的 `max_parallel_steps`;配置为 1 时行为等同串行。
3. 任一 step 失败后,依赖它的未开始 step 被 skipped,run 最终 failed;已在执行的独立 step 可完成或被取消,但最终状态必须一致。
4. 用户中断 run 时,所有 in-flight provider 调用收到取消信号,未开始 step 为 skipped,run 收敛 interrupted。
5. run_events seq 仍单调递增,前端不会因为并发事件乱序而丢失最终状态。
6. 并发执行不破坏 artifact selection 和 cost ledger:每个 step 的 artifact/cost 归属到正确 run_step。

## 验收标准

- [ ] 菱形图中 B/C 并发执行,总耗时明显低于串行基线。
- [ ] `max_parallel_steps=1` 时测试表现为串行。
- [ ] 并发 run 中断后无悬挂 running step。
- [ ] 并发下事件 seq 单调且 workspace state 最终一致。

## 边界情况

- 同一 provider 对并发有限流:限流失败按 provider 错误处理,不无限重试。
- cache hit step 视为立即完成,可以释放下游依赖。
- 依赖图 cycle 仍由 GraphService validate 阻止。

## 发布说明

新增运行配置项,默认值应保守。依赖 GH-58 后台执行和 GH-62 cache 行为稳定后实施。
