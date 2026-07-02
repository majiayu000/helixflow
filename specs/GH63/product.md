# Product Spec

## Linked Issue

GH-63

## 用户问题

执行计划当前是拓扑排序后的串行列表。无依赖分支本可以同时执行,但用户要等待串行总时长。图里常见的多候选图、多分支处理应能按依赖并发执行,同时保持事件、状态和中断一致。

## 目标

- executor 按依赖就绪度调度,无依赖节点并发执行。
- 并发度可配置并有安全默认值。
- run events / step 状态在并发下保持一致、可重放/可对账。
- interrupt 能取消所有 in-flight 节点并收敛到终态。

## 非目标

- 不做跨 run 调度。
- 不做多机/队列集群执行。
- 不改变 provider API 的单步调用契约。

## Behavior Invariants

1. 对菱形图 `A -> B`, `A -> C`, `B/C -> D`,B 和 C 在 A 成功后可并发执行,D 必须等待 B/C 都完成。
2. 并发度达到配置上限后,额外 ready steps 排队,不得无限 spawn。
3. 任一步失败时,依赖它的下游 step 不执行,run 进入 failed 或 interrupted 的明确终态。
4. interrupt 会通知/取消所有 in-flight provider calls,未开始下游 step 标记 skipped/interrupted。
5. event ordering 必须可对账:同一 step 内 state transitions 有序,workspace state 最终反映 store 事实。
6. 与 GH-62 cache 共存:cache hit step 可立即完成并释放其下游依赖。

## 验收标准

- [ ] 菱形图中 B/C 并发,总时长低于串行基线。
- [ ] 并发度设置为 1 时行为等价串行执行。
- [ ] 中断并发 run 后无 hanging step/provider task。

## 边界情况

- 一个并发分支失败,另一个仍在执行。
- cached step 与 running step 混合。
- provider 不支持主动取消,只能让 run 忽略迟到结果。

## 发布说明

并发执行可能改变 event interleaving,但不改变最终 run output 语义。默认并发度需保守。

