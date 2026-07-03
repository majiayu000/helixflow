# Task Plan

## Linked Issue

GH-89

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

| ID | Owner | Depends on | Task | Done when | Verification |
| --- | --- | --- | --- | --- | --- |
| SP89-T0 | maintainer | none | 完成 spec review 和 implementation readiness gate | `GH89` 的 product/tech/tasks 被维护者接受，并推进到 `ready_to_implement` | `python3 checks/route_gate.py --repo . --route implement --issue 89 --state ready_to_implement --json` |
| SP89-T1 | frontend | SP89-T0, SP88-T4 | 添加 client op submit helper 和 idempotency key 复用 | 每次 retry 复用同一 key，accepted op 更新 `canvas.seq` | `cd web && npm test -- app.test.tsx` |
| SP89-T2 | frontend | SP89-T1 | 实现 durable node add/move/resize | 操作提交为 `node_add`/`node_move`/`node_resize` 并可 reload 恢复 | `cd web && npm test -- app.test.tsx` |
| SP89-T3 | frontend | SP89-T1 | 实现 inspector `node_patch` | conflict-sensitive params 携带 `prev`，失败显示错误且 state 可恢复 | `cd web && npm test -- app.test.tsx` |
| SP89-T4 | frontend | SP89-T1 | 实现 durable edge add/delete | connect/delete 写入 `edge_add`/`edge_delete` 并可 reload 恢复 | `cd web && npm test -- app.test.tsx` |
| SP89-T5 | frontend/backend | SP89-T2-T4 | 对齐 optimistic reconcile 和 server validation | stale/conflict/validation error 不污染 durable local state | `cargo test -p helixflow-graph && cargo test -p helixflow-server && cd web && npm test` |
| SP89-T6 | qa | SP89-T5 | 完成 build 和交互回归 | add/move/resize/patch/connect/delete reload 后仍存在 | `cd web && npm run build` |

## 并行拆分

- Frontend op lane owns `web/src/api.ts`, `web/src/store.ts`.
- Canvas interaction lane owns `web/src/components/graph-canvas.tsx`.
- Backend alignment lane owns only the current canvas route/module if required.
- Test lane owns touched web tests and focused Rust tests.

## 验证

- `cd web && npm test`
- `cd web && npm run build`
- `cargo test -p helixflow-graph`
- `cargo test -p helixflow-server`

## Handoff Notes

GH-89 depends on GH-88 canvas source-of-truth state. Keep comments and run UI out
of scope unless a small shared reducer change is necessary.
