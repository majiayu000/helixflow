# Task Plan: Versioned GraphCanvas Node Repositioning

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/44
Locale: zh-CN

## Scope

Implement one focused PR for versioned node repositioning: node/group drag in `GraphCanvas`, explicit save layout action, backend layout save route, version/history persistence, and deterministic tests. Keep box selection, shortcuts, clipboard, manual graph editing, and agent canvas ops out of this issue.

## Tasks

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP44-T1 | backend | none | Add layout save request validation and route wiring. | `POST /api/workspaces/{workspace_id}/layout` rejects malformed input, unknown nodes, stale base version, and pending proposal. | `cargo test --workspace` |
| SP44-T2 | backend | SP44-T1 | Persist layout changes as a new manual graph version. | Valid layout save updates only `GraphNode.pos`, creates a new current version, and returns refreshed `WorkbenchState`. | `cargo test --workspace` |
| SP44-T3 | frontend | none | Add layout draft helper functions and tests. | Single-node and multi-node position updates are computed from immutable base nodes and stable output order. | `cd web && npm test -- app.test.tsx` |
| SP44-T4 | frontend | SP44-T3 | Add node selection and node/group drag in `GraphCanvas`. | Dragging a node updates rendered node positions and edge paths without changing `graph` prop; selected nodes move together. | `cd web && npm test -- app.test.tsx` |
| SP44-T5 | frontend | SP44-T3-SP44-T4 | Add save layout API/store/App wiring. | Dirty layout shows save control, successful save refreshes state and clears draft, failure reports a system error. | `cd web && npm test -- app.test.tsx` |
| SP44-T6 | frontend | SP44-T4-SP44-T5 | Add pending proposal UI guard. | Pending proposal preview disables layout drag/save while selection and review display still work. | `cd web && npm test -- app.test.tsx` |
| SP44-T7 | verification | SP44-T1-SP44-T6 | Run deterministic verification and inspect diff. | Fresh checks pass and changed files match GH44 scope. | `cd web && npm run build`; `cargo check --workspace`; `cargo test --workspace`; `git diff --check` |

## Thread Ownership

- Backend implementation lane owns `crates/server/src/layout_routes.rs`, `crates/server/src/main.rs`, and backend tests in the same route module.
- Frontend implementation lane owns `web/src/components/graph-canvas.tsx`, `web/src/components/graph-canvas-layout.ts`, `web/src/app.tsx`, `web/src/api.ts`, `web/src/store.ts`, `web/src/app.test.tsx`, and `web/src/canvas.css`.
- Read-only review lane owns route/version/proposal risk review and must not edit files.
- Coordinator owns SpecRail artifacts, final verification, PR gate, merge, and next-issue handoff.

## Handoff Notes

- Do not implement box select, keyboard shortcuts, clipboard, node delete, edge creation, param editing, or agent ops in this PR.
- Keep request/response JSON keys camelCase at the TypeScript boundary.
- Keep Rust internal fields snake_case.
- Do not silently ignore invalid save requests; return an error and leave current version unchanged.
- Do not allow layout save while a pending proposal exists.
- User has explicitly authorized commit, PR creation, and merge after SpecRail gate and verification.
