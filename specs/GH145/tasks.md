# Task Plan

## Linked Issue

GH-145

## Spec Packet

- Product: `specs/GH145/product.md`
- Tech: `specs/GH145/tech.md`

## 实现任务

- [ ] `SP145-T1` 阶段 A：GraphNode 内嵌语义 + WorkflowGraphV2 合并 + 消费方切换 + serde 兼容/等价测试。Owner: graph/compiler/run/server/web 单执行者串行。Done when: 旧图读取/迁移/幂等测试全绿；`WorkflowGraphV2` 删除。Verify: `cargo test --workspace && cd web && npm test`
- [ ] `SP145-T2` 阶段 A：capability 全栈改名 + 旧 run 重试显式失败 + cache key v4。Owner: 同上。Done when: 生产代码 `image_generate` 仅剩 `canonical_capability` 恒等映射；重试失败测试绿。Verify: `cargo test --workspace`
- [ ] `SP145-T3` 阶段 B（gate：#144 迁移证据）：删 `canonical_capability` + 零残留 rg 断言 + 可选 0007 清列。Owner: coordinator。Done when: `rg image_generate crates/ web/src`（除注释/fixture）零命中。Verify: `cargo test --workspace`

## 验证

- [ ] `SP145-T4` 每阶段交付前全量复跑并附会话内输出。Owner: coordinator。Done when: 对应阶段 Verify 全过。Verify: `cargo test --workspace`

## Handoff Notes

- 关键发现：`GraphNode` 现为 `deny_unknown_fields` —— 阶段 A 部署前的旧服务读不了
  内嵌语义的新图；revert 安全窗口 = 首个新图写入前。单机本地部署风险低，但顺序
  约束必须保留在发布说明。
- `versions.semantics_json` 保留为派生索引（#144 迁移证据查询依赖它计数）。
