# Product Spec

## Linked Issue

GH-93

## 用户问题

Canvas-agent 流程需要继续兼容 graph-only workspace，并用确定性回归覆盖证明
open/edit/comment/run/result/reload 全链路。缺少覆盖会让后续迁移容易回归。

## 目标

- 添加 graph-only workspace bootstrap compatibility tests。
- 添加 snapshot replay tests，覆盖 store seq ahead of snapshot seq。
- 添加 canvas edits reload persistence tests。
- 添加 E2E regression coverage 或确定性 manual verification script。
- 行为稳定后更新 docs。

## 非目标

- 不新增 provider integration。
- 不做 canvas-agent 范围外的 UI redesign。
- 不替代单元/集成测试，只补齐端到端验收路径。

## Behavior Invariants

1. 只有旧 graph 数据的 workspace 可作为有效 canvas 打开。
2. Store seq ahead of snapshot seq 时 replay missing ops。
3. Snapshot seq ahead of store seq 时明确报错，不静默降级。
4. 完整故事覆盖 open、add、move、connect、comment、run、approve、artifact、reload。
5. Repo deterministic checks 在该兼容层变更后通过。

## 验收标准

- [ ] Existing graph-only workspace opens as a valid canvas.
- [ ] Snapshot replay catches up missing ops.
- [ ] Full canvas-agent story passes: open, add node, move node, connect node, add comment, run, approve, artifact appears, reload preserves state.
- [ ] All repo deterministic checks pass.

## 边界情况

- Legacy graph 数据缺字段时使用显式错误或兼容转换，不显示伪数据。
- Manual verification script 必须确定性，不依赖真实 provider 调用。
- Docs 更新必须反映当前行为，不承诺未实现 provider 或 identity 功能。

## 发布说明

该变更为 canvas-agent 迁移提供兼容性和回归证明；可作为后续 release gate 的基础。
