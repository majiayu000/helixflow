# Task Plan: Seed Sweep Run Plans And Recommendations

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/28
Locale: zh-CN

## Scope

Implement one PR for seed sweep planning, confirmation payload/UI, grouped confirmation/hold routes, and selected recommendation persistence. Keep real provider billing, AI quality scoring, arbitrary batch scripting, and full group progress UI out of this PR.

## Tasks

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP28-T1 | backend | none | Add seed-capable sweep plan helper and video mock `seed` schema support. | Seed sweep request creates validated variants for current graph without provider invoke. | `cargo test -p helixflow-server workbench_message`; `cargo test -p helixflow-run sweep` |
| SP28-T2 | backend | SP28-T1 | Extend pending confirmation payload with sweep metadata. | Response includes run count, pending changes, total estimate, and interruptible status. | `cargo test -p helixflow-server workbench_message` |
| SP28-T3 | backend | SP28-T1 | Add group run lookup and grouped confirm/hold routes. | Confirm executes all sweep runs and selects recommendation; hold cancels all waiting group runs. | `cargo test -p helixflow-server run_routes` |
| SP28-T4 | frontend | SP28-T2 | Extend web schemas and ConfirmModal display. | Modal renders optional run count, pending changes, and interruptible status. | `cd web && npm test -- app.test.tsx`; `cd web && npm run build` |
| SP28-T5 | verification | SP28-T1-SP28-T4 | Run deterministic verification and PR gate. | Fresh local checks pass; PR body records evidence; review-thread state is clean before merge. | full verification matrix |

## Thread Ownership

- Planner/reviewer lanes are read-only and inspect route/spec/diff evidence.
- Backend lane owns `crates/store/src/sweep_records.rs`, `crates/store/src/lib.rs`, `crates/registry/src/lib.rs`, `crates/server/src/sweep_support.rs`, `crates/server/src/workbench_payload.rs`, `crates/server/src/workbench_message.rs`, `crates/server/src/run_routes.rs`, and focused Rust tests.
- Frontend lane owns `web/src/types.ts`, `web/src/components/run-panels.tsx`, and `web/src/app.test.tsx`.
- Coordinator owns full verification, PR creation, independent review, PR gate, merge, and remote closure audit.

## Handoff Notes

- Do not call provider invoke before confirmation.
- Do not add batch scripting or user-supplied executable plans.
- Do not expose local storage paths in outputs.
- Keep `run_records.rs` and `workbench_message.rs` under control by adding helper modules instead of expanding large files.
