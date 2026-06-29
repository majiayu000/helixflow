# Task Plan: Workbench Run Queue And Interrupt Controls

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/23
Locale: zh-CN

## Scope

Implement one PR for direct workbench Queue and active run Interrupt. Keep undo/restore, export, output selection, failure cards, and seed sweep in separate issues.

## Tasks

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP23-T1 | backend | none | Add `POST /api/workspaces/{workspace_id}/runs` route and handler. | Handler loads current graph, executes manual run through `RunService`, and returns typed run payload. | `cargo test -p helixflow-server run_routes` |
| SP23-T2 | backend | SP23-T1 | Add `POST /api/runs/{run_id}/interrupt` route and handler. | Active run can be interrupted; missing/inactive/terminal run returns explicit error. | `cargo test -p helixflow-run interrupt`; `cargo test -p helixflow-server run_routes` |
| SP23-T3 | backend | SP23-T1 | Define first-version duplicate-run policy. | Repeated Queue cannot double invoke provider from one obvious double-submit path. | `cargo test -p helixflow-server run_routes` |
| SP23-T4 | frontend | SP23-T1, SP23-T2 | Add API client and store actions for Queue and Interrupt. | Store applies returned run/output state and exposes errors. | `cd web && npm test -- app.test.tsx` |
| SP23-T5 | frontend | SP23-T4 | Wire Queue and Interrupt controls in the workbench UI. | Queue calls run route, Interrupt calls interrupt route, confirmation modal behavior remains unchanged. | `cd web && npm test -- app.test.tsx`; `cd web && npm run build` |
| SP23-T6 | verification | SP23-T1-SP23-T5 | Run deterministic verification. | Fresh commands pass and outputs are recorded in PR body. | `cargo fmt --check`; `cargo test -p helixflow-run`; `cargo test -p helixflow-server run_routes`; `cd web && npm test -- app.test.tsx`; `cd web && npm run build`; `git diff --check` |

## Thread Ownership

- Backend lane owns `crates/server/src/run_routes.rs`, `crates/server/src/main.rs`, and focused run/server tests.
- Frontend lane owns `web/src/api.ts`, `web/src/store.ts`, UI control files, and `web/src/app.test.tsx`.
- Lanes must not edit the same file concurrently. If a shared type/schema file is required, pause and assign it to one lane.

## Handoff Notes

- This task depends on current workbench UI structure from PR #22. If #22 is not merged, implement this as a stacked PR targeting `spec/gh21-prompt-stack` or wait until #22 lands.
- Do not implement output selection, export, or version restore in this PR.
- Do not route Queue through chat messages.
