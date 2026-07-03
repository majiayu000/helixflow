# Tech Spec

## Linked Issue

GH-90

## Product Spec

Link to `product.md`.

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Store | `web/src/store.ts` | Canvas state can hold comments/presence. | Needs durable comment and volatile presence reducers. |
| Canvas UI | `web/src/components/graph-canvas.tsx`, `web/src/canvas.css` | Node/edge interaction surface exists. | Needs comment affordance and cursor/selection overlays. |
| API/WS | `web/src/api.ts`, server canvas routes/ws | Backend has comments and presence foundations. | Needs frontend calls and event consumption. |
| Tests | `web/src/app.test.tsx`, server tests | Existing coverage does not prove collaboration UX. | Must cover comment persistence and presence volatility. |

## 设计方案

Add UI affordances for comments on nodes, edges, and canvas positions. Submit
comment mutations as durable canvas ops. Track collaborator presence separately
from `CanvasDocument`, update it from REST/WS volatile messages, and render
cursor/selection overlays without persisting them.

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 | comment op helpers and UI | `npm test`, `cargo test -p helixflow-server` |
| P2 | comment reducer/rendering | `npm test` |
| P3 | presence API/WS handling | `npm test` |
| P4 | snapshot/event restore | `npm test` |
| P5 | multi-client presence fixture | `npm test` |

## 数据流

Durable path: UI comment intent -> canvas op endpoint -> snapshot/event replay.
Volatile path: selection/cursor/viewport -> presence endpoint or WS -> frontend
presence map only.

## 备选方案

- Store presence as canvas ops: rejected because presence is volatile and should
  not pollute durable history.

## 风险

- Security: actor display fields must be rendered as text, not HTML.
- Compatibility: comment UI must not block graph-only fallback.
- Performance: cursor updates should be throttled.
- Maintenance: keep presence separate from durable reducer.

## 测试计划

- [ ] Unit tests: comment reducer and presence reducer.
- [ ] Integration tests: comment reload and WS presence rendering.
- [ ] Manual verification: two browser clients show selection/cursor updates.

## 回滚方案

Disable comment/presence UI while preserving backend durable comment records.
