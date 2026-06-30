# Task Plan: Manual Graph Proposal Editing Controls

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/46
Locale: zh-CN

## Scope

Deliver one focused PR that adds manual pending proposal creation through bounded UI controls and backend validation. Keep direct graph mutation, run execution, JSON graph editing, schema migrations, and large editor libraries out of scope.

## Tasks

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP46-T1 | backend | none | Add registry catalog route. | Frontend can fetch builtin node definitions without duplicating catalog data. | `cargo test --workspace` |
| SP46-T2 | backend | SP46-T1 | Add manual proposal request types and creation route. | Route maps add/remove/connect/disconnect/set-param to `ProposalOp`, rejects stale or concurrent pending proposals, and persists preview proposal. | `cargo test --workspace` |
| SP46-T3 | frontend | SP46-T1 | Add catalog/manual proposal API types and store action. | Store can create a manual proposal and surfaces request errors. | `cd web && npm test -- app.test.tsx` |
| SP46-T4 | frontend | SP46-T3 | Add `ManualProposalPanel` and wire it into `App`. | UI supports add node, remove node, add edge, remove edge, set param, and disables while pending proposal exists. | `cd web && npm test -- app.test.tsx` |
| SP46-T5 | verification | SP46-T1-SP46-T4 | Run deterministic verification and inspect diff. | Fresh checks pass and existing apply/dismiss behavior remains intact. | `cd web && npm run build`; `cargo fmt --check`; `cargo check --workspace`; `cargo test --workspace`; `git diff --check` |

## Thread Ownership

- Backend implementation lane owns `crates/server/src/proposal_routes.rs`, `crates/server/src/main.rs`, and Rust route tests.
- Frontend implementation lane owns `web/src/types.ts`, `web/src/api.ts`, `web/src/store.ts`, `web/src/app.tsx`, `web/src/components/manual-proposal-panel.tsx`, CSS, and `web/src/app.test.tsx`.
- Read-only review thread owns architecture and omission review only; it must not edit files.
- Coordinator owns SpecRail artifacts, final verification, PR gate, merge, and next-issue handoff.

## Handoff Notes

- Use `GraphService.preview_proposal()` as the backend validation gate.
- Do not create a current graph version until existing apply route runs.
- Do not silently ignore invalid JSON, invalid params, invalid edges, stale versions, or existing pending proposals.
- Keep GH40 provider catalog work out of scope; GH46 uses the node registry catalog only.
