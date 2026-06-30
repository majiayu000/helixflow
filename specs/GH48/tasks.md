# Task Plan: Split GraphCanvas Components And Improve Large-Graph Rendering

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/48
Locale: zh-CN

## Scope

Implement one focused PR for `GraphCanvas` maintainability and baseline rendering performance. Split existing code into clear component/helper modules and replace per-node run step scans with memoized lookup maps. Do not add new user-facing editor features.

## Tasks

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP48-T1 | frontend | none | Move node card, inspector, edge SVG, minimap, and rendering lookup helpers into focused modules. | `graph-canvas.tsx` keeps orchestration only and remains below 400 lines. | `wc -l web/src/components/graph-canvas.tsx` |
| SP48-T2 | frontend | SP48-T1 | Replace per-node `run.steps.find` with memoized `buildRunStepStateMap`. | Node render uses O(1) step lookup per node. | `cd web && npm test -- app.test.tsx` |
| SP48-T3 | frontend | SP48-T1-SP48-T2 | Preserve existing behavior through composition. | Toolbar, nodes, edges, minimap, inspector, proposal diff, layout dirty/save render remain present. | `cd web && npm test -- app.test.tsx` |
| SP48-T4 | frontend | SP48-T2 | Add large graph lookup helper tests. | Tests cover `buildNodeMap`, `buildRunStepStateMap`, and edge signature set behavior. | `cd web && npm test -- app.test.tsx` |
| SP48-T5 | verification | SP48-T1-SP48-T4 | Run deterministic verification and inspect diff. | Fresh checks pass and no backend/API behavior is changed. | `cd web && npm run build`; `cargo check --workspace`; `cargo test --workspace`; `git diff --check` |

## Thread Ownership

- Frontend implementation lane owns `web/src/components/graph-canvas.tsx`, new `graph-canvas-*` component/helper files, and `web/src/app.test.tsx`.
- Read-only review lane owns split-boundary and performance hotspot review; it must not edit files.
- Coordinator owns SpecRail artifacts, final verification, PR gate, merge, and next-issue handoff.

## Handoff Notes

- Do not change `GraphCanvas` props unless required by TypeScript after splitting.
- Do not add culling, box selection, keyboard shortcuts, clipboard, manual graph editing, or agent ops.
- Keep GH42/GH44 helper exports stable for tests.
- User has explicitly authorized commit, PR creation, and merge after SpecRail gate and verification.
