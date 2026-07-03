# Task Plan

## Linked Issue

GH-91

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

| ID | Owner | Depends on | Task | Done when | Verification |
| --- | --- | --- | --- | --- | --- |
| SP91-T0 | maintainer | none | 完成 spec review 和 implementation readiness gate | `GH91` 的 product/tech/tasks 被维护者接受，并推进到 `ready_to_implement` | `python3 checks/route_gate.py --repo . --route implement --issue 91 --state ready_to_implement --json` |
| SP91-T1 | frontend/backend | SP91-T0, SP88-T4 | canvas Run action 提交 `run_request` | 用户从 canvas 发起 run，并进入现有 cost confirmation | `cargo test -p helixflow-server && cd web && npm test` |
| SP91-T2 | frontend | SP91-T1 | 保留 cost confirmation modal 行为 | approve/cancel/reject 语义与现有 run flow 一致 | `cd web && npm test -- app.test.tsx` |
| SP91-T3 | frontend/backend | SP91-T1 | run events 更新 canvas node runtime | run step 状态映射到对应 canvas node，失败显示错误 | `cargo test -p helixflow-server && cd web && npm test` |
| SP91-T4 | frontend/backend | SP91-T3 | 渲染 `artifact_attach` result state | artifacts attach 到来源 node，重复 attach 不重复 artifact ids | `cargo test --workspace && cd web && npm test` |
| SP91-T5 | frontend | SP91-T4 | 复用 artifact preview metadata | image/video/text/json artifact 可从 canvas inspect | `cd web && npm test -- app.test.tsx && npm run build` |

## 并行拆分

- Canvas run UI lane owns `web/src/components/graph-canvas.tsx`, `web/src/store.ts`.
- Artifact UI lane owns `web/src/components/artifact-stage.tsx`.
- Backend lane owns server/run files only if event payloads need node id data.
- Test lane owns web app tests and focused Rust run/server tests.

## 验证

- `cargo test --workspace`
- `cd web && npm test`
- `cd web && npm run build`

## Handoff Notes

Do not add provider integrations. Keep cost gate behavior identical to the
existing run flow.
