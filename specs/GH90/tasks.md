# Task Plan

## Linked Issue

GH-90

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

| ID | Owner | Depends on | Task | Done when | Verification |
| --- | --- | --- | --- | --- | --- |
| SP90-T0 | maintainer | none | 完成 spec review 和 implementation readiness gate | `GH90` 的 product/tech/tasks 被维护者接受，并推进到 `ready_to_implement` | `python3 checks/route_gate.py --repo . --route implement --issue 90 --state ready_to_implement --json` |
| SP90-T1 | frontend | SP90-T0, SP88-T4 | 添加 node/edge/position comment affordance | 用户能从节点、边或 canvas 位置进入 comment 创建流程 | `cd web && npm test -- app.test.tsx` |
| SP90-T2 | frontend/backend | SP90-T1 | 实现 `comment_add`/`comment_patch`/`comment_delete` UI flows | comments 可新增、编辑、resolve、删除，并从 snapshot/events 恢复 | `cargo test -p helixflow-server && cd web && npm test` |
| SP90-T3 | frontend | SP90-T0 | 发送 selection/cursor/viewport presence | presence 走 volatile channel，不写入 durable `canvas_ops` | `cd web && npm test -- app.test.tsx` |
| SP90-T4 | frontend/backend | SP90-T3 | 消费 WS presence events 并渲染 collaborator overlays | 另一个 client 发送 presence 时可见 cursor/selection | `cargo test -p helixflow-server && cd web && npm test` |
| SP90-T5 | qa | SP90-T2, SP90-T4 | 完成 reload 和 volatility 回归 | reload 恢复 comments，presence 不进入 op history | `cd web && npm run build` |

## 并行拆分

- Comment UI lane owns `web/src/components/graph-canvas.tsx`, `web/src/canvas.css`.
- Presence lane owns `web/src/store.ts`, `web/src/api.ts`.
- Backend lane owns server canvas/ws files only if current route shape is insufficient.
- Test lane owns focused web/server tests.

## 验证

- `cd web && npm test`
- `cd web && npm run build`
- `cargo test -p helixflow-server`

## Handoff Notes

Presence must remain volatile. Do not reuse durable canvas op history for cursor
or selection state.
