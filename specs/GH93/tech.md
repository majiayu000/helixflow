# Tech Spec

## Linked Issue

GH-93

## Product Spec

Link to `product.md`.

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Store/migration | `crates/store/migrations/*`, server canvas module | Existing workspace data must remain loadable. | Needs compatibility and seq replay coverage. |
| Frontend tests | `web/src/app.test.tsx` and related tests | Web tests cover selected flows. | Needs full canvas-agent regression path. |
| Checks/scripts | optional `scripts/` or `checks/` | Deterministic checks exist for workflow. | May host manual verification script if E2E is not available. |
| Docs | docs under `docs/` | Canvas docs exist. | Must reflect final behavior. |

## 设计方案

Add compatibility tests for graph-only bootstrap and snapshot/op replay ordering.
Add web regression coverage or a deterministic manual script for the full
canvas-agent story using local fixtures/stubs instead of real provider calls.
Update docs only after behavior is verified.

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 | server/store compatibility tests | `cargo test --workspace` |
| P2 | replay tests | `cargo test --workspace` |
| P3 | error handling tests | `cargo test --workspace` |
| P4 | web E2E/manual script | `npm test`, manual script |
| P5 | deterministic repo checks | `cargo test --workspace`, `npm test`, `npm run build` |

## 数据流

Legacy graph workspace -> bootstrap canvas -> snapshot plus op replay -> web
regression flow -> artifact/result verification through deterministic fixtures.

## 备选方案

- Rely on manual QA only: rejected because migration compatibility requires
  repeatable regression evidence.

## 风险

- Security: fixtures must not contain secrets.
- Compatibility: tests should preserve existing graph-only behavior.
- Performance: full regression should remain deterministic and reasonably fast.
- Maintenance: avoid brittle UI timing in E2E/manual script.

## 测试计划

- [ ] Unit tests: replay ordering and error conditions.
- [ ] Integration tests: graph-only bootstrap and reload persistence.
- [ ] Manual/E2E: full canvas-agent story using local deterministic fixtures.

## 回滚方案

Keep compatibility tests and disable only the unstable E2E/manual runner until
the underlying feature tranche is fixed.
