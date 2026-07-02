# Product Spec

## Linked Issue

GH-62

## 用户问题

当前每次 run 都按执行计划全量重跑整图。用户只改一个末端参数时,上游昂贵节点仍会再次调用 provider,既慢又浪费成本。类 ComfyUI 的核心体验是只重跑受影响的脏子图,干净节点复用已有产物并在 UI 中可见。

## 目标

- 每个节点基于输入 hash、上游 artifact hash、参数、node_type 和 provider 形成稳定缓存 key。
- run 时干净节点跳过 provider 调用,复用缓存 artifact,并在 run step/UI 中标记 cached。
- 改中游参数时,只让该节点及下游子图失效。
- 用户可选择强制重跑,绕过缓存。

## 非目标

- 不做跨 workspace 共享缓存。
- 不做磁盘配额、自动清理策略或缓存管理 UI。
- 不改变 graph version/hash 的语义。

## Behavior Invariants

1. 同一 workspace、同一 provider、同一 node_type、同一参数和同一上游内容再次运行时,节点可命中缓存且不调用 provider。
2. 修改末端节点参数时,未受影响的上游节点命中缓存;修改中游节点参数时,该节点及所有下游节点重跑。
3. provider id 是缓存 key 的一部分;同图切换 provider 不复用另一 provider 的产物。
4. cache hit 的 run step 仍出现在本次 run 中,状态为 succeeded,并可被 UI 标记为 cached。
5. cache hit 复用的 artifact 在本次 run outputs 中可预览/下载,但不伪造 provider 调用或实际成本。
6. 用户启用 force rerun 时,本次 run 不读取缓存,成功后刷新缓存记录。
7. 缓存缺失、缓存 artifact 文件丢失或校验 hash 不匹配时,节点按 cache miss 处理并正常执行,不能返回损坏产物。

## 验收标准

- [ ] 只改末端节点参数重跑时,上游节点全部 cache hit 且 provider invoke count 不增加。
- [ ] 改中游参数时,只有受影响子图重跑。
- [ ] force rerun 生效并刷新缓存。
- [ ] UI 能区分 cached step 与本次真实执行 step。

## 边界情况

- 上游节点返回多个 artifact:缓存 key 必须覆盖所有被下游消费的 artifact content hash。
- provider 失败的节点不写入成功缓存。
- cache hit 需要写入本次 run 的 artifact 关联,否则刷新页面后 outputs 会丢失。

## 发布说明

需要 SQLite migration 增加节点缓存记录。旧 run/artifact 不回填缓存,新 run 成功后逐步建立缓存。
