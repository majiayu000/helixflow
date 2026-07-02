# Tech Spec

## Linked Issue

GH-63

## Product Spec

`specs/GH63/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Run executor | `crates/run/src/lib.rs` | steps 线性 for 循环执行 | 需要改为 ready queue scheduler |
| Graph plan | `crates/graph/src/lib.rs` | compile_plan 输出拓扑序 steps | 需要保留依赖边/输入引用用于调度 |
| Run events | store run records/events | 后台 run 已有事件与 interrupt 语义(GH-58) | 并发下要保持一致性 |
| Cache | GH-62 node cache | cached step 可跳过 provider | cached completion 参与依赖释放 |

## 设计方案

### 1. Scheduler

把线性执行循环替换为 DAG scheduler。初始化每个 step 的 dependency count 和 downstream list。ready queue 存放依赖为 0 的 step。worker pool 按 `max_concurrency` 拉取 ready step,执行完成后释放下游。

### 2. Concurrency config

server config / run options 增加 `max_node_concurrency`,默认 2 或 4,最小 1。值为 1 时走同一 scheduler,保证串行兼容路径仍被测试覆盖。

### 3. Failure and interrupt

任一 step failed 时,run 标记 failed,未开始且依赖失败的 steps 标记 skipped。interrupt 通过 GH-58 的 interrupt token 广播给所有 in-flight tasks;provider 不可取消时,迟到结果在 run 已终止后被丢弃且记录 debug event。

### 4. Event consistency

每个 step 状态写入仍经 store helper,同一 step transition 串行。事件 payload 包含 step_id/status/timestamp;前端用 workspace state refetch 对账,不依赖全局严格序号渲染最终事实。

### 5. Cache integration

调度器执行 step 时先调用 GH-62 cache gate。cache hit 立即产生 cached/succeeded transition,释放下游依赖;cache miss 进入 provider call。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 B/C 并发,D 等待 | scheduler DAG | fake provider barrier timing test |
| P2 并发度上限 | worker pool config | max=1 串行,max=2 并发测试 |
| P3 fail skips downstream | failure propagation | failing branch test |
| P4 interrupt all in-flight | interrupt token fanout | two slow providers interrupt test |
| P5 event/state consistency | store events + state fetch | final state assertions |
| P6 cache hit releases deps | cache gate integration | cached upstream/downstream test |

## 数据流

compiled plan -> dependency graph -> ready queue -> bounded worker pool -> step transition events/store writes -> downstream release -> terminal run state.

## 备选方案

- 每个 step 一个 unbounded task:容易资源爆炸,放弃。
- 只并发 provider calls,状态仍串行等待:不能释放依赖与中断,放弃。

## 风险

- Security: 并发不新增 secret 面,但日志需避免 provider payload 泄露。
- Compatibility: event interleaving 变化,前端需以 state 对账为准。
- Performance: 高并发可能打爆 provider rate limit,需要配置上限。
- Maintenance: scheduler 状态机复杂,需要集中测试。

## 测试计划

- [ ] Unit tests: dependency graph / ready queue / failure propagation。
- [ ] Integration tests: fake provider timing、interrupt、max concurrency。
- [ ] Manual verification: 菱形图运行时长与 UI 状态。

## 回滚方案

将 `max_node_concurrency` 设为 1 或通过 feature flag 切回串行 scheduler。

