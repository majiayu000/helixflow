# Task Plan: GraphCanvas Viewport Persistence, Wheel Zoom, And Minimap

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/42
Locale: zh-CN

## Scope

Implement one focused PR for `GraphCanvas` navigation: workspace-scoped viewport persistence, mouse-anchored wheel zoom, and minimap navigation. Keep node edit persistence, manual graph editing, backend graph schema changes, and third-party canvas library adoption out of this issue.

## Tasks

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP42-T1 | frontend | none | Wire `workspaceId` into `GraphCanvas` and add safe viewport persistence helpers. | Viewport loads/saves per workspace id; invalid localStorage data falls back safely. | `cd web && npm test -- app.test.tsx` |
| SP42-T2 | frontend | SP42-T1 | Add mouse-anchored wheel zoom and shared zoom clamp helper. | Wheel zoom keeps pointer world coordinate stable and respects min/max zoom. | `cd web && npm test -- app.test.tsx` |
| SP42-T3 | frontend | SP42-T1 | Add minimap rendering from current `drawGraph`. | Minimap displays node distribution and current viewport when graph has nodes; empty graph is safe. | `cd web && npm test -- app.test.tsx` |
| SP42-T4 | frontend | SP42-T3 | Add minimap click/drag navigation. | Pointer interaction on minimap updates main canvas viewport without changing graph data. | `cd web && npm test -- app.test.tsx` |
| SP42-T5 | frontend | SP42-T1-SP42-T4 | Add focused tests for viewport restore, wheel zoom, minimap navigation, and pending proposal preview. | Tests fail without the new behavior and pass with implementation. | `cd web && npm test -- app.test.tsx` |
| SP42-T6 | verification | SP42-T1-SP42-T5 | Run deterministic verification and inspect diff. | Fresh commands pass and no unrelated files are changed. | `cd web && npm run build`; `cargo check --workspace`; `git diff --check` |

## Thread Ownership

- Frontend implementation lane owns `web/src/components/graph-canvas.tsx`, `web/src/app.tsx`, `web/src/styles.css`, and `web/src/app.test.tsx`.
- Read-only review lane owns scope/risk review and must not edit files.
- Coordinator owns SpecRail artifacts, final verification, and PR handoff.

## Handoff Notes

- Do not implement node drag persistence in this PR.
- Do not implement manual edge creation in this PR.
- Do not modify Rust graph/server/store code unless a build break proves a type contract needs a narrow frontend-facing adjustment.
- Do not persist viewport to backend in this PR.
- Keep minimap based on `drawGraph` so pending proposal preview remains visible during review.
