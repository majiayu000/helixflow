# Tech Spec

## Linked Issue

GH-62

## Product Spec

`specs/GH62/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Run execution | `crates/run/src/lib.rs` | 编译后的 steps 线性执行,每步都调用 provider | cache gate 应插在 step invoke 前 |
| Graph hashing | `crates/graph/src/lib.rs` | graph hash 用于 version 记录 | 节点级 key 需更细粒度 |
| Artifacts | `crates/store`, `crates/server/src/workspace_state.rs` | artifacts 按 run/step 持久化并展示 | cache hit 需要复用 artifact payload |
| UI run steps | `web/src/store.ts`, run step components | step 状态展示 queued/running/succeeded/failed | 增加 cached 状态或 cached badge |

## 设计方案

### 1. Cache key

新增 node execution cache key:`sha256(node_type + provider + normalized params + sorted input port artifact hashes + runtime-relevant options)`. 上游 artifact hash 使用已持久化 content hash;缺失 hash 时该节点不可 cache hit。

### 2. Store schema

新增 node cache table,记录 workspace_id、node_id、cache_key、artifact ids、created_at、provider、graph_version_id。首版仅 workspace-local。写入和读取都通过 store helper,避免 run executor 直接拼 SQL。

### 3. Executor gate

RunService 在执行每个 step 前检查 force rerun flag。非 force 且 cache hit/artifacts 可读时,创建 step result 为 cached/succeeded,不调用 provider。miss 或 stale entry 时正常 invoke provider,成功后写 cache entry。

### 4. UI/state

run step payload 增加 `cached: boolean` 或 `status: cached` 的兼容表示。前端在 step 行展示 cached badge,artifact preview 仍来自 output/artifact payload。

### 5. Force rerun

手动 run/agent run request 增加 force rerun 开关,默认 false。force true 时跳过 cache read,但成功结果仍写 cache。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 key 覆盖输入/provider | cache key builder | unit tests 改 params/provider/upstream hash key 均变化 |
| P2 末端改参上游命中 | RunService cache gate | integration test provider call count |
| P3 中游改参只重跑下游 | graph dependency + cache gate | diamond/chain graph integration test |
| P4 UI cached 标记 | workspace/run payload + web | payload snapshot + web test |
| P5 force rerun | request flag + executor | force true provider call count |
| P6 artifact 缺失重跑 | cache validation | missing artifact file test |

## 数据流

run request -> compile plan -> for each ready step compute cache key -> store lookup -> validate artifact content -> cached step payload or provider invoke -> persist artifact -> write cache entry -> workspace state exposes cached flag.

## 备选方案

- 只按 graph_hash 缓存整图:无法做到脏子图,放弃。
- 跨 workspace cache:权限、secret/provider 输入与磁盘配额复杂,放到后续。

## 风险

- Security: cache key 不应包含 secret 明文。
- Compatibility: cached 状态需前端兼容旧 succeeded step。
- Performance: key 构建和 artifact hash 读取不能阻塞整个 run。
- Maintenance: cache invalidation 要集中在 helper,避免散落逻辑。

## 测试计划

- [ ] Unit tests: cache key builder、store helper。
- [ ] Integration tests: chain/diamond graph provider call count、missing artifact rerun、force rerun。
- [ ] Manual verification: UI cached badge 与 artifact preview。

## 回滚方案

禁用 cache read gate,保留写入表不使用;已生成 artifact 仍按普通 run artifact 展示。

