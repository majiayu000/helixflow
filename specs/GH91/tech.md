# Tech Spec

## Linked Issue

GH-91

## Product Spec

Link to `product.md`.

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Canvas UI | `web/src/components/graph-canvas.tsx` | Canvas has node UI but run flow is incomplete. | Needs run action and node runtime rendering. |
| Artifacts | `web/src/components/artifact-stage.tsx` | Existing preview metadata is available. | Should be reused for canvas artifact inspect. |
| Store/API | `web/src/store.ts`, `web/src/api.ts` | Workbench/run state exists. | Needs canvas runtime and artifact attachment updates. |
| Backend/run | server canvas route, `crates/run/src/lib.rs` if needed | Backend can project canvas and attach artifacts. | Event payload may need node id alignment. |
| Tests | Web and Rust run/server tests | Existing tests cover run behavior outside full canvas story. | Must prove canvas path compatibility. |

## 设计方案

Wire the canvas Run control to submit `run_request` while preserving the existing
cost confirmation path. Apply run events and `artifact_attach` ops to canvas node
runtime/artifact state. Reuse artifact preview metadata for selected result
nodes or result strips.

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 | run request UI/cost gate | `npm test`, `cargo test --workspace` |
| P2 | runtime event reducer | `npm test` |
| P3 | backend attach idempotency | `cargo test -p helixflow-server` |
| P4 | artifact render/inspect | `npm test` |
| P5 | reload persistence | `npm test`, Rust server/store tests |

## 数据流

Canvas run action -> durable `run_request` op -> existing run service/cost gate
-> run events -> canvas node runtime updates -> `artifact_attach` ops -> canvas
artifact rendering.

## 备选方案

- Launch runs from graph-only state: rejected because canvas must be the visible
  source of truth for canvas-agent workflows.

## 风险

- Security: artifact URLs/metadata must remain within existing preview safety.
- Compatibility: existing graph run flow and cost modal must keep working.
- Performance: artifact previews should reuse existing lazy loading.
- Maintenance: avoid branching provider logic in the frontend.

## 测试计划

- [ ] Unit tests: run request payload and runtime reducer.
- [ ] Integration tests: artifact attach and reload persistence.
- [ ] Manual verification: run from canvas with cost confirmation.

## 回滚方案

Disable canvas Run control and keep existing graph/run flow.
