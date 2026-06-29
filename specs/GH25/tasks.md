# Task Plan: Output Selection And Artifact Preview

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/25
Locale: zh-CN

## Scope

Implement one PR for output selection persistence and safe ArtifactStage preview. Keep seed sweep recommendation UI, binary storage, signed URLs, and full thumbnail generation in separate issues.

## Tasks

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP25-T1 | backend | none | Add single-selection store helper for run artifacts. | Selecting one artifact clears selected on sibling artifacts in the same run. | `cargo test -p helixflow-store run_records` |
| SP25-T2 | backend | SP25-T1 | Add output select/preview/download routes. | Select returns workspace state; latest-run guard and safe payload are enforced. | `cargo test -p helixflow-server artifact_routes` |
| SP25-T3 | backend | SP25-T2 | Enhance output payload contract. | `storageUri` is safe route; preview is lightweight and optional. | `cargo test -p helixflow-server artifact_routes` |
| SP25-T4 | frontend | SP25-T2 | Add API/store output selection action. | Store replaces state from server selection response. | `cd web && npm test -- app.test.tsx` |
| SP25-T5 | frontend | SP25-T4 | Wire OutputsStrip and ArtifactStage. | Clicking output selects it; selected preview renders safely. | `cd web && npm test -- app.test.tsx`; `cd web && npm run build` |
| SP25-T6 | verification | SP25-T1-SP25-T5 | Run deterministic verification. | Fresh commands pass and outputs are recorded in PR body. | `cargo fmt --check`; `cargo check --workspace`; `cargo test --workspace`; `cd web && npm test -- app.test.tsx`; `cd web && npm run build`; `git diff --check` |

## Thread Ownership

- Backend lane owns `crates/store/src/run_records.rs`, `crates/server/src/artifact_routes.rs`, `crates/server/src/main.rs`, `crates/server/src/workbench_payload.rs`, `crates/server/src/workspace_state.rs`, and focused tests.
- Frontend lane owns `web/src/api.ts`, `web/src/store.ts`, `web/src/app.tsx`, `web/src/components/run-panels.tsx`, `web/src/components/artifact-stage.tsx`, `web/src/types.ts`, and `web/src/app.test.tsx`.
- Coordinator owns full verification and PR gate.

## Handoff Notes

- Do not expose raw provider/local storage paths to frontend.
- Do not inline large artifact bytes in workspace state.
- Do not implement seed sweep recommendation UI in this PR.
