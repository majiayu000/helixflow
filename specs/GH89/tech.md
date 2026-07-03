# Tech Spec

## Linked Issue

GH-89

## Product Spec

Link to `product.md`.

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Client API | `web/src/api.ts` | Canvas endpoints are typed at the boundary. | Needs op submit helper and response handling. |
| Store | `web/src/store.ts` | Canvas state must reconcile accepted ops. | Owns optimistic and server-confirmed state. |
| Canvas UI | `web/src/components/graph-canvas.tsx` | User edit actions exist in UI. | Must emit durable ops instead of only local edits. |
| Backend canvas | `crates/server/src/workbench_canvas.rs` or current canvas routes | Backend validates and records canvas ops. | May need small response/validation shape alignment. |
| Tests | `web/src/app.test.tsx`, Rust canvas/server tests | Existing tests cover graph/workbench behavior. | Must prove reload and conflict behavior. |

## 设计方案

Introduce a client op helper that creates or reuses idempotency keys per edit
attempt. UI edit handlers submit `node_add`, `node_move`, `node_resize`,
`node_patch`, `edge_add`, and `edge_delete`. Accepted ops update `canvas.seq`
and the local reducer. Validation errors become visible UI errors and do not
commit optimistic state as durable truth.

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 | op helper and edit handlers | `npm test` |
| P2 | idempotency helper | `npm test`, `cargo test -p helixflow-server` |
| P3 | inspector patch payload | `npm test` |
| P4 | error/reconcile path | `npm test` |
| P5 | backend snapshot/op replay | `cargo test -p helixflow-graph`, `cargo test -p helixflow-server` |

## 数据流

Input: user edit intent. Output: `CanvasOpEnvelope` sent to backend. Persistence:
backend `canvas_ops` log and snapshot replay. External calls: local REST canvas
op endpoint only.

## 备选方案

- Keep local-only edits and batch-save later: rejected because it silently loses
  sync/conflict semantics and violates CanvasDocument source-of-truth.

## 风险

- Security: idempotency keys must not include secrets.
- Compatibility: graph-only fallback must continue to render.
- Performance: high-frequency move/resize may need throttling or commit-on-drop.
- Maintenance: avoid duplicating backend op semantics in unrelated components.

## 测试计划

- [ ] Unit tests: client op payloads and reducer behavior.
- [ ] Integration tests: reload persistence and stale/conflict errors.
- [ ] Rust tests: graph/server op acceptance and idempotency.

## 回滚方案

Disable durable edit handlers behind the existing graph fallback while leaving
server op validation untouched.
