# Product Spec

## Linked Issue

GH-62

## 用户问题

当前每次 run 都顺序重跑整张图。用户只改一个末端参数时,上游节点也会重新调用 provider,既慢又浪费成本。用户需要类似 ComfyUI 的脏子图执行:未受影响节点复用已有产物,只重跑受影响下游。

## 目标

- 每个节点按输入内容生成稳定 cache key。
- 干净节点复用既有 artifact,UI 明确标记 cached。
- 改中游参数时仅该节点及其下游重跑。
- 用户可以显式强制重跑,绕过缓存。

## 非目标

- 不跨 workspace 共享缓存。
- 不做磁盘配额、LRU 清理或远端缓存服务。
- 不改变 graph versioning 语义。

## Behavior Invariants

1. cache key 必须包含 node type、provider、params、上游 artifact/content hash 和相关 graph input,避免错误复用。
2. 只改末端节点参数时,上游节点 cache hit,不得发起 provider 调用。
3. 改中游参数时,该节点与所有依赖它的下游节点重跑,不依赖它的分支可复用缓存。
4. cache hit 的 step 在 run steps/UI 中显示 cached 状态,并指向复用 artifact。
5. force rerun 对当前 run 禁用缓存读取,但可写入新的 cache entry。
6. 缓存读取失败或缓存 artifact 缺失时,该节点必须重跑,不得返回缺失数据。

## 验收标准

- [ ] 只改末端节点参数重跑时,上游 provider 调用次数为 0,UI 显示 cached。
- [ ] 改中游参数只重跑受影响子图。
- [ ] force rerun 生效并产生新的 provider 调用。
- [ ] cache artifact 缺失时自动重跑并修复缓存。

## 边界情况

- provider selection 改变使 cache key 改变。
- 上游 artifact 存在 metadata 但 content 文件缺失。
- 并发两个 run 命中同一缺失 cache entry。

## 发布说明

这是运行时行为优化。需要在 UI 中标记 cached,避免用户误以为节点正在重新生成。

