# Product Spec

## Linked Issue

GH-89

## 用户问题

Canvas add/move/resize/patch/connect/delete 仍未完整转成持久化 backend
`canvas_ops`。用户编辑如果只停留在本地 UI，会在 reload、retry 或多端同步时丢失。

## 目标

- 添加 client op helper 和 idempotency key 复用。
- 实现 node add/move/resize/patch。
- 实现 edge add/delete。
- 将 optimistic state 与 accepted server ops 对齐。
- 显示 server validation/conflict 错误，且不污染本地 state。

## 非目标

- 不实现 comment UX；该范围属于 GH-90。
- 不实现 run/artifact UI；该范围属于 GH-91。
- 不改变 provider/run service 架构。

## Behavior Invariants

1. 每个用户提交的 durable edit 都对应一个 backend `canvas_op`。
2. Retry 使用同一个 idempotency key，不产生重复 op。
3. Inspector 对 conflict-sensitive params patch 时携带 `prev`。
4. Server 拒绝 stale/conflicting op 时，UI 显示错误并保持可恢复。
5. Reload 后 add/move/resize/patch/connect/delete 结果仍存在。

## 验收标准

- [ ] Add/move/resize/patch/connect/delete survive page reload.
- [ ] Inspector param patch sends `prev` for conflict-sensitive fields.
- [ ] Retry reuses idempotency key and does not duplicate ops.
- [ ] Stale/conflicting op returns visible error and local state is recoverable.

## 边界情况

- Backend op validation error 必须进入 error state，不能 warning 后假成功。
- Optimistic update 必须能被 accepted op 或 refetch 纠正。
- Delete node 时依赖 edge 行为必须遵循 backend 规则。

## 发布说明

该变更使 canvas 编辑成为 durable source-of-truth 编辑，但 comments、presence
和 run/artifact backfill 仍由后续 issue 完成。
