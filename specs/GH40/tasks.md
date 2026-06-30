# Task Plan: Runtime Provider Catalog And State

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/40
Locale: zh-CN

## Scope

Implement one M3 foundation PR that exposes backend-owned provider catalog/status through workspace state, writes safe provider/backend/connector catalogs into graph-mode agent sessions, and updates the TopBar to render provider status from backend state. Do not implement real Atlas/OpenAI/ComfyUI provider calls, BYOK UI, provider marketplace, or manual provider selection in this issue.

## Tasks

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP40-T1 | backend | none | Define provider catalog/status payload and safe projection for `RuntimeProvider`. | Mock and unavailable providers can produce serializable provider state without secrets. | `cargo test -p helixflow-gateway`; `cargo test -p helixflow-server app_state` |
| SP40-T2 | backend | SP40-T1 | Add provider catalog/status to workspace state. | `GET /api/workspaces/{id}/state` includes `providers` for default mock and unavailable env provider cases. | `cargo test -p helixflow-server workspace_state` |
| SP40-T3 | agent | SP40-T1 | Write safe workflow backend, runtime provider, and API connector catalogs into graph-mode agent ctx. | Proposal-mode sessions contain the three catalog files and no provider secrets; chat mode keeps the existing lightweight contract. | `cargo test -p helixflow-agent` |
| SP40-T4 | frontend | SP40-T2 | Update web state schemas and TopBar provider status rendering. | Frontend consumes backend `providers`, does not fabricate mock provider truth, and no longer contains `Atlas 已配置` / `Atlas 未配置`. | `cd web && npm test -- app.test.tsx`; `cd web && npm run build`; `git grep -n "Atlas 已配置\\|Atlas 未配置" -- web/src && exit 1 || true` |
| SP40-T5 | verification | SP40-T1-SP40-T4 | Run full regression and live smoke for mock and unavailable provider states. | Fresh local checks pass; PR body records exact commands and state payload evidence. | `cargo fmt --check`; `cargo check --workspace`; `cargo test --workspace`; `git diff --check`; manual curl smoke |

## Thread Ownership

- Planner/reviewer lanes are read-only and inspect issue, specs, implementation diff, verification logs, and PR state.
- Backend lane owns provider catalog projection, `app_state.rs`, `workspace_state.rs`, `workbench_payload.rs`, and related Rust tests.
- Agent lane owns `crates/agent/src/lib.rs`, prompt/session context generation, and agent contract tests.
- Frontend lane owns `web/src/types.ts`, `web/src/components/top-bar.tsx`, `web/src/app.test.tsx`, and any small state wiring needed for provider status display.
- Coordinator owns integration, full verification, PR creation, review-thread handling, merge gate, and remote closure audit.

## Handoff Notes

- Do not make unavailable providers fall back to mock.
- Do not expose credentials, raw auth headers, signed URLs, local secret paths, or env var values in state or agent ctx.
- Do not add real external provider calls in this issue.
- Keep mock provider available and clearly labeled for local development and tests.
- Keep existing proposal, run confirmation, seed sweep, output preview, export, undo, and restore behavior unchanged.
