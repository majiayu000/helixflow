# Task Plan: Workbench Version Undo And Restore

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/24
Locale: zh-CN

## Scope

Implement one PR for workspace version undo and history restore. Keep redo, graph diff viewer, import/export changes, output preview, failure diagnosis, and seed sweep in separate issues.

## Tasks

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP24-T1 | backend | none | Add undo and restore version routes. | Routes create `restore` version records and return latest workspace state. | `cargo test -p helixflow-server version_routes` |
| SP24-T2 | backend | SP24-T1 | Map stale proposal apply to conflict. | Pending proposal based on old version returns `409 Conflict`. | `cargo test -p helixflow-server proposal_routes` |
| SP24-T3 | frontend | SP24-T1 | Add API client and store actions. | Undo/restore update store state from backend response and surface failures. | `cd web && npm test -- app.test.tsx` |
| SP24-T4 | frontend | SP24-T3 | Wire TopBar Undo and HistoryPanel Restore actions. | Buttons call the new APIs and avoid restoring current version. | `cd web && npm test -- app.test.tsx`; `cd web && npm run build` |
| SP24-T5 | verification | SP24-T1-SP24-T4 | Run deterministic verification. | Fresh commands pass and outputs are recorded in PR body. | `cargo fmt --check`; `cargo check --workspace`; `cargo test --workspace`; `cd web && npm test -- app.test.tsx`; `cd web && npm run build`; `git diff --check` |

## Thread Ownership

- Backend lane owns `crates/server/src/version_routes.rs`, `crates/server/src/main.rs`, `crates/server/src/proposal_routes.rs`, and focused server tests.
- Frontend lane owns `web/src/api.ts`, `web/src/store.ts`, `web/src/app.tsx`, `web/src/components/top-bar.tsx`, `web/src/components/run-panels.tsx`, `web/src/types.ts`, and `web/src/app.test.tsx`.
- Coordinator owns full verification and PR gate.

## Handoff Notes

- Do not delete or mutate previous version records.
- Do not change proposal dismiss behavior.
- Do not create migrations unless the existing `restore` source proves insufficient.
