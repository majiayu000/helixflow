# Task Plan: Failed Run Diagnosis Cards And Minimal Fix Proposals

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/27
Locale: zh-CN

## Scope

Implement one PR for failed run diagnosis payloads, ErrorCard UI, and DebugWorkflow verification. Keep automatic retry, seed sweep, provider execution changes, and broad secret-redaction policy out of this PR.

## Tasks

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP27-T1 | backend | none | Add structured run/step error payloads to workspace state. | Failed run state includes bounded `error.summary` and hidden-capable `raw` for run and failed steps. | `cargo test -p helixflow-server workspace_state` |
| SP27-T2 | backend | SP27-T1 | Strengthen DebugWorkflow tests. | “修复报错” uses latest failed run context and leaves current version unchanged before apply. | `cargo test -p helixflow-server workbench_message` |
| SP27-T3 | frontend | SP27-T1 | Extend web run types and websocket error updates. | `run.error` and `step.error` parse from state and update from events. | `cd web && npm test -- app.test.tsx` |
| SP27-T4 | frontend | SP27-T3 | Add ChatPane ErrorCard. | Failed run renders summary card; raw error is absent until expanded. | `cd web && npm test -- app.test.tsx`; `cd web && npm run build` |
| SP27-T5 | verification | SP27-T1-SP27-T4 | Run deterministic verification. | Fresh commands pass and PR body records evidence. | full verification matrix |

## Thread Ownership

- Backend lane owns `crates/server/src/workspace_state.rs`, `crates/server/src/workbench_message.rs`, and specs.
- Frontend lane owns `web/src/types.ts`, `web/src/store.ts`, `web/src/app.tsx`, `web/src/components/chat-pane.tsx`, `web/src/styles.css`, and `web/src/app.test.tsx`.
- Coordinator owns full verification, PR creation, independent review, and merge gate.

## Handoff Notes

- Do not expose raw error in the normal assistant message body.
- Do not auto-apply DebugWorkflow proposals.
- Do not change provider invocation or retry behavior.
