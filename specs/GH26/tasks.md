# Task Plan: Workbench Workflow JSON Export

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/26
Locale: zh-CN

## Scope

Implement one PR for current-version workflow JSON export. Keep pending preview export, import, ComfyUI conversion, undo/restore, output preview, failure cards, and seed sweep in separate issues.

## Tasks

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP26-T1 | backend | none | Add `GET /api/versions/{version_id}/export` route. | Route reads server-owned version graph and returns `WorkflowGraph` JSON. | `cargo test -p helixflow-server version_routes` |
| SP26-T2 | backend | SP26-T1 | Add export safety check. | Unsafe secret/header/runtime/path fields reject with explicit error and no unsafe payload. | `cargo test -p helixflow-server version_routes` |
| SP26-T3 | frontend | SP26-T1 | Add API client and store export action. | Store calls version export API for current `workspace.versionId` and parses `WorkflowGraphSchema`. | `cd web && npm test -- app.test.tsx` |
| SP26-T4 | frontend | SP26-T3 | Wire TopBar Export download. | Export button is enabled for current version and downloads backend response JSON. | `cd web && npm test -- app.test.tsx`; `cd web && npm run build` |
| SP26-T5 | verification | SP26-T1-SP26-T4 | Run deterministic verification. | Fresh commands pass and outputs are recorded in PR body. | `cargo fmt --check`; `cargo check --workspace`; `cargo test --workspace`; `cd web && npm test -- app.test.tsx`; `cd web && npm run build`; `git diff --check` |

## Thread Ownership

- Backend lane owns `crates/server/src/version_routes.rs`, `crates/server/src/main.rs`, and focused server tests.
- Frontend lane owns `web/src/api.ts`, `web/src/store.ts`, `web/src/app.tsx`, `web/src/components/top-bar.tsx`, and `web/src/app.test.tsx`.
- Coordinator owns shared verification and PR gate.

## Handoff Notes

- Do not export `pendingProposal.previewGraph` in this PR.
- Do not silently redact unsafe graph content; reject unsafe export until a future explicit redaction UX exists.
- Do not change run queue, proposal apply, or workspace state semantics.
