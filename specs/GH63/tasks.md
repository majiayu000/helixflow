# Task Plan

## Linked Issue

GH-63

## Spec Packet

- Product: `specs/GH63/product.md`
- Tech: `specs/GH63/tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP63-T1` | graph/run lane | GH58 | 为 ExecutionPlan 构建 dependency DAG helper。 | 菱形图依赖关系和 cycle 既有校验测试通过。 | `cargo test -p helixflow-graph && cargo test -p helixflow-run dag` |
| `SP63-T2` | run lane | `SP63-T1` | 用 ready queue + semaphore coordinator 替换顺序 step 循环。 | parallel=1 串行、parallel>1 并发、事件 seq 单调。 | `cargo test -p helixflow-run parallel` |
| `SP63-T3` | run lane | `SP63-T2` | 实现并发失败传播和共享 interrupt 取消。 | 失败下游 skipped,中断取消所有 in-flight。 | `cargo test -p helixflow-run interrupt parallel` |
| `SP63-T4` | server lane | `SP63-T2` | 接入 `max_parallel_steps` 配置并覆盖集成测试。 | 配置值影响 run executor,默认保守。 | `cargo test -p helixflow-server parallel` |

## 并行拆分

graph DAG helper 可先行;run scheduler 与 interrupt/failure 需要同一 lane 串行;server 配置在 run API 稳定后接入。

## 验证

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP63-T5` | verification lane | `SP63-T1`-`SP63-T4` | 全仓测试和慢节点冒烟。 | 菱形图并发收益与中断收敛记录在 PR。 | `cargo check --workspace && cargo test --workspace && cd web && npm test && npm run build` |

## Handoff Notes

- event seq 分配必须集中在 coordinator。
- 并发测试使用 blocking fake provider,不要用 sleep 判断并发。
