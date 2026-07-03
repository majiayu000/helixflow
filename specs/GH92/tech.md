# Tech Spec

## Linked Issue

GH-92

## Product Spec

Link to `product.md`.

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Server routes | `crates/server/src/main.rs`, current canvas/ws module | Canvas WS exists without dedicated ticket gate. | Needs ticket endpoint and optional enforcement. |
| Frontend API | `web/src/api.ts` | Canvas API calls exist. | Needs ticket fetch and event catch-up calls. |
| Store | `web/src/store.ts` | Canvas state can track seq. | Needs reconnect/gap recovery state. |
| Tests | server/web tests | No ticket/reconnect regression coverage yet. | Must prove rejection, fallback, and catch-up. |

## 设计方案

Add a short-lived local canvas ticket endpoint. Gate WS by ticket only when ticket
mode is enabled by explicit env config. Track last accepted seq in frontend
canvas state. On reconnect or detected gap, fetch events after the last seq and
apply them before accepting new WS events. Keep REST fallback available when WS
is offline.

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 | server ticket gate | `cargo test -p helixflow-server` |
| P2 | env config | `cargo test -p helixflow-server` |
| P3 | frontend reconnect flow | `npm test` |
| P4 | gap recovery | `npm test`, server tests |
| P5 | idempotency/retry | `npm test`, server tests |

## 数据流

Ticket path: client POST ticket -> server returns opaque ticket -> WS connects
with ticket -> server validates when enforcement is enabled. Sync path: last seq
-> events fetch -> ordered reducer -> resume WS.

## 备选方案

- Require ticket in all environments immediately: rejected because local dev
  compatibility is an explicit acceptance criterion.

## 风险

- Security: no hardcoded secrets; ticket validation must fail closed when enabled.
- Compatibility: ticket-disabled local mode must remain easy to run.
- Performance: reconnect should fetch only missing events.
- Maintenance: keep ticket logic local and isolated from future identity work.

## 测试计划

- [ ] Unit tests: ticket config and validation.
- [ ] Integration tests: reconnect catch-up and seq gap recovery.
- [ ] Manual verification: WS offline fallback through REST fetch.

## 回滚方案

Disable ticket enforcement through env config and keep REST snapshot/events path.
