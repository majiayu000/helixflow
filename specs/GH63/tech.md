# Tech Spec

## Linked Issue

GH-63

## Product Spec

`specs/GH63/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Graph plan | `crates/graph/src/lib.rs` | `compile_plan` 产出线性 `steps`,graph validation 已阻止 cycle | 并发调度需要从 graph edges 构建 step dependency DAG |
| Run executor | `crates/run/src/lib.rs` | `execute_created_run` 顺序 `for` 循环执行 step | 需要替换为 ready queue + join set/semaphore 调度 |
| Interrupt | `crates/run/src/lib.rs` | `RunInterrupt` 可通过 `tokio::select!` 取消单个 provider 调用 | 并发时多个 in-flight step 要共享同一 interrupt |
| Events/store | `crates/store/src/run_records.rs`, `EventBus` | event seq 按 run 递增写入 | 多任务并发写事件必须由单一 sequencer 序列化 |

## 设计方案

在 `ExecutionPlan` 中保留线性 steps,同时新增依赖索引 helper:根据 graph edge 的 to/from node 找出每个 step 的 upstream step ids 和 downstream ids。RunService 用 ready queue 调度:依赖计数为 0 的 step 入队,通过 `tokio::sync::Semaphore` 限制并发数,每个 step task 只负责执行 provider/builtin 和返回结果;状态写入、event seq 分配、downstream 解锁由 coordinator 任务串行处理。

`HELIXFLOW_MAX_PARALLEL_STEPS` 或 server config 提供并发度,默认 2 或 4 中较保守值;值为 1 时使用同一 scheduler 但只发一个 permit。所有 step 共享 run-level `RunInterrupt`;中断触发后 coordinator 停止发新 step,通知所有 in-flight task,并把未开始 step 标记 skipped。

失败策略:一旦某 step failed,coordinator 标记依赖它的未开始下游为 skipped,run 最终 failed。已在执行且不依赖失败 step 的任务可自然完成,但 run 终态仍 failed;如果实现选择主动取消全部 in-flight step,必须保证状态可解释并有测试。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1、P2 | dependency DAG + semaphore scheduler | run 单测:菱形图 B/C 同时 blocked,parallel=1 串行 |
| P3 | failure propagation | run 单测:失败 step 下游 skipped,run failed |
| P4 | shared interrupt | run 单测:两个 in-flight provider 被同一 interrupt 取消 |
| P5 | event sequencer | store/run 测试:并发事件 seq 单调 |
| P6 | artifact/cost attribution | run 测试:并发 artifacts/cost 归属正确 step |

## 数据流

compile plan -> build dependency DAG -> ready queue -> acquire semaphore -> spawn step task -> coordinator receives result -> persist step/artifact/cost/event -> release downstream -> terminal state。

## 备选方案

- 为每个 provider 建独立队列:被否,本 issue 只做单 run 内依赖并发。
- 让每个 step task 自行写 event seq:被否,会引入 seq 竞争和乱序风险。

## 风险

- Security: 无新增外部输入面。
- Compatibility: 并发可能暴露 provider 限流,默认并发度必须保守。
- Performance: 大图 ready queue 需要避免 busy loop。
- Maintenance: 并发测试不得依赖脆弱 sleep,应使用 blocking fake provider 同步点。

## 测试计划

- [ ] Unit tests: DAG 依赖、parallel=1 串行、parallel>1 并发、失败传播、中断取消。
- [ ] Integration tests: server config 注入并发度,workspace state 最终一致。
- [ ] Manual verification: 慢 mock provider 菱形图总耗时低于串行基线。

## 回滚方案

将 `max_parallel_steps` 固定为 1 或 revert scheduler 即恢复串行执行;无 schema 迁移。
