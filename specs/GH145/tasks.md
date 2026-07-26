# Task Plan

## Linked Issue

GH-145

## Spec Packet

- Product: `specs/GH145/product.md`
- Tech: `specs/GH145/tech.md`

## 实现任务

- [x] `SP145-T1`（2026-07-27 会话内验证：`cargo test --workspace` 453 passed / 0 failed；`cd web && npx tsc --noEmit && npm test` 184 passed / 21 files；`legacy_graph_json_migrates_and_roundtrips_idempotently` 覆盖旧图 JSON 读取→迁移→写出→再读幂等；`WorkflowGraphV2` 已删除，`crates/graph/src/semantics.rs` 承接 validate/migrate；新增 `ProposalOp::SetSemantics` 保证 kept 节点语义收敛）阶段 A：GraphNode 内嵌语义 + WorkflowGraphV2 合并 + 消费方切换 + serde 兼容/等价测试。Owner: graph/compiler/run/server/web 单执行者串行。Done when: 旧图读取/迁移/幂等测试全绿；`WorkflowGraphV2` 删除。Verify: `cargo test --workspace && cd web && npm test`
- [x] `SP145-T2`（2026-07-27 会话内验证：`rg image_generate` 生产代码仅剩 `canonical_capability` 恒等映射两处（rust/web，阶段 B 删）；atlas/fal/mock 三 provider legacy 串显式 `UnsupportedCapability` 测试绿；cache key `schemaVersion` 3→4）阶段 A：capability 全栈改名 + 旧 run 重试显式失败 + cache key v4。Owner: 同上。Done when: 生产代码 `image_generate` 仅剩 `canonical_capability` 恒等映射；重试失败测试绿。Verify: `cargo test --workspace`
- [x] `SP145-T3` 阶段 B（gate：#144 迁移证据）：删 `canonical_capability` + 零残留 rg 断言 + 可选 0007 清列。Owner: coordinator。Done when: `rg image_generate crates/ web/src`（除注释/fixture）零命中。Verify: `cargo test --workspace`

## 验证

- [x] `SP145-T4` 每阶段交付前全量复跑并附会话内输出。Owner: coordinator。Done when: 对应阶段 Verify 全过。Verify: `cargo test --workspace`

## Handoff Notes

- 阶段 A 已交付（2026-07-27）。**部署顺序硬约束**：旧二进制 `GraphNode` 为
  `deny_unknown_fields`，读不了内嵌语义的新图文件——必须先部署本版本，再允许任何
  新图写入；revert 安全窗口 = 首个内嵌语义版本写入前。
- run 侧语义加载改为 `version_semantics(graph, store, version_id)`：图内嵌优先，
  `versions.semantics_json` 列回退（#144 迁移证据查询仍按列计数，列=派生索引）。
- 阶段 B（SP145-T3）gate 不变：等 #144 存量迁移证据后删 `canonical_capability`
  （rust `crates/graph/src/semantics.rs` + web `model-catalog-tray.tsx` 各一处）。

- 关键发现：`GraphNode` 现为 `deny_unknown_fields` —— 阶段 A 部署前的旧服务读不了
  内嵌语义的新图；revert 安全窗口 = 首个新图写入前。单机本地部署风险低，但顺序
  约束必须保留在发布说明。
- `versions.semantics_json` 保留为派生索引（#144 迁移证据查询依赖它计数）。
