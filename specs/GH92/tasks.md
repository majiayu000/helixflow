# Task Plan

## Linked Issue

GH-92

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

| ID | Owner | Depends on | Task | Done when | Verification |
| --- | --- | --- | --- | --- | --- |
| SP92-T0 | maintainer | none | 完成 spec review 和 implementation readiness gate | `GH92` 的 product/tech/tasks 被维护者接受，并推进到 `ready_to_implement` | `python3 checks/route_gate.py --repo . --route implement --issue 92 --state ready_to_implement --json` |
| SP92-T1 | backend | SP92-T0 | 添加 `POST /api/canvases/{canvas_id}/ticket` | endpoint 返回短期 opaque ticket，错误不泄露 secret/token | `cargo test -p helixflow-server` |
| SP92-T2 | backend | SP92-T1 | 添加 ticket-mode env config 与 WS gate | enforcement enabled 时 missing/invalid ticket 被拒绝，disabled 时本地兼容 | `cargo test -p helixflow-server` |
| SP92-T3 | frontend | SP92-T2 | 客户端记录 last accepted `seq` 并 reconnect fetch events | reconnect 先请求 `/events?afterSeq={seq}` 再恢复 WS apply | `cd web && npm test -- app.test.tsx` |
| SP92-T4 | frontend/backend | SP92-T3 | 检测 seq gap 并通过 event fetch 恢复 | gap 未补齐前不应用后续 WS event | `cargo test -p helixflow-server && cd web && npm test` |
| SP92-T5 | frontend | SP92-T4 | 保留 REST fallback | WS offline 时通过 REST fetch/polling 不腐蚀 state | `cd web && npm test -- app.test.tsx && npm run build` |

## 并行拆分

- Backend ticket lane owns server route/ws files.
- Frontend sync lane owns `web/src/api.ts`, `web/src/store.ts`.
- Test lane owns focused server and web tests.

## 验证

- `cargo test -p helixflow-server`
- `cd web && npm test`
- `cd web && npm run build`

## Handoff Notes

Respect fail-closed config behavior. Do not hardcode ticket secrets or introduce
remote identity assumptions.
