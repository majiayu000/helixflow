# Product Spec

## Linked Issue

GH-90

## 用户问题

后端已有 durable comments 和 volatile `canvas_presence`，但 UI 还不能创建、
编辑、resolve comments，也不能显示协作者 cursor/selection。协作状态缺失会让
canvas-agent workspace 缺少 review 和协作闭环。

## 目标

- 支持 node/edge/position comment creation。
- 支持 comment edit/resolve/delete。
- 发送 selection/cursor/viewport presence。
- 消费 canvas WebSocket presence events。
- 渲染 collaborator cursors 和 selections。

## 非目标

- 不实现全文 CRDT。
- 不接入远程 identity provider。
- 不把 presence 写入 durable `canvas_ops`。

## Behavior Invariants

1. Comments 通过 durable `comment_add`、`comment_patch`、`comment_delete` 保存。
2. Resolved comments 在删除前仍可查询和恢复显示。
3. Presence 仅走 volatile `/presence` 或 WebSocket message。
4. Reload 后 comments 从 snapshot/events 恢复。
5. 另一个 client 发送 presence 时，当前 UI 能显示 cursor/selection。

## 验收标准

- [ ] Comments persist and reload from backend canvas snapshot/events.
- [ ] Presence updates do not create durable `canvas_ops`.
- [ ] Closing/reopening the page restores comments.
- [ ] WebSocket presence updates are visible when another client sends them.

## 边界情况

- WebSocket 断开时 presence 可丢弃，但 durable comments 不可丢失。
- Unknown actor 或缺少 display name 时使用稳定 fallback，不阻塞渲染。
- Comment mutation 失败必须显示错误，不静默假成功。

## 发布说明

该变更补齐协作 UX；身份和 CRDT 不在本轮发布范围。
