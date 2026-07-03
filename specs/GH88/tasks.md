# Task Plan

## Linked Issue

GH-88

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

| ID | Owner | Depends on | Task | Done when | Verification |
| --- | --- | --- | --- | --- | --- |
| SP88-T0 | maintainer | none | 完成 spec review 和 implementation readiness gate | `GH88` 的 product/tech/tasks 被维护者接受，并推进到 `ready_to_implement` | `python3 checks/route_gate.py --repo . --route implement --issue 88 --state ready_to_implement --json` |
| SP88-T1 | frontend | SP88-T0 | 补齐 typed canvas snapshot fetch | workspace canvas API 有类型化请求/响应，错误不静默吞掉 | `cd web && npm test -- app.test.tsx` |
| SP88-T2 | frontend | SP88-T1 | 添加或补齐 frontend canvas slice | store 记录 canvas status/error/document/selection/presence 基础字段 | `cd web && npm test -- app.test.tsx` |
| SP88-T3 | frontend | SP88-T2 | workspace hydration 拉取 canvas snapshot | 打开 workspace 会请求并保存后端 canvas snapshot | `cd web && npm test -- app.test.tsx` |
| SP88-T4 | frontend | SP88-T3 | `GraphCanvas` 优先从 `CanvasDocument` 渲染 | 有 canvas document 时 nodes/edges 来自 canvas，graph-only workspace 仍 fallback | `cd web && npm test -- app.test.tsx` |
| SP88-T5 | frontend | SP88-T4 | 保留 proposal preview 行为 | preview/review/apply/dismiss 不把 preview state 当 durable canvas state | `cd web && npm test -- app.test.tsx && npm run build` |

## 并行拆分

- Frontend state/API lane owns `web/src/store.ts`, `web/src/api.ts`, `web/src/types.ts`.
- Canvas render lane owns `web/src/components/graph-canvas.tsx`.
- Test lane owns `web/src/app.test.tsx`.

## 验证

- `cd web && npm test`
- `cd web && npm run build`

## Handoff Notes

This is the first implementation tranche for `specs/canvas-agent-full`. Do not
start durable editing ops from GH-89 until GH-88 source-of-truth behavior is in
place or explicitly bundled with it.
