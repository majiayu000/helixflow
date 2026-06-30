# Task Plan: Agent Bounded Canvas Ops Contract

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/47
Locale: zh-CN

## Scope

Implement one focused PR for the internal bounded canvas ops contract. Keep external MCP publication, direct UI automation, direct destructive graph mutation, and selected-subgraph execution out of this issue.

## Tasks

| ID | Owner | Dependencies | Task | Done When | Verification |
| --- | --- | --- | --- | --- | --- |
| SP47-T1 | agent | none | Add canvas ops/context schema and ctx file generation. | Graph-mode sessions write compact state and contract files; unknown fields are rejected. | `cargo test -p helixflow-agent canvas` |
| SP47-T2 | agent | SP47-T1 | Update prompt stack for bounded canvas ops. | Prompt includes allowed ops and maps layout/graph/run actions to proposal or confirmation gates. | `cargo test -p helixflow-agent prompt` |
| SP47-T3 | server | SP47-T1 | Accept frontend selection context and preserve proposal gate. | Message route forwards filtered selection and rejects new proposal when a pending proposal exists. | `cargo test -p helixflow-server workbench_message` |
| SP47-T4 | frontend | none | Lift canvas selection and include it in Agent messages. | `GraphCanvas` reports selected ids; `sendWorkspaceMessage` payload includes `canvasContext.selection.nodeIds`. | `cd web && npm test -- app.test.tsx` |
| SP47-T5 | frontend | SP47-T4 | Improve canvas ops evidence display. | `agent_log:canvas_ops` appears in the tool log area with stable label/title. | `cd web && npm test -- app.test.tsx` |
| SP47-T6 | fullstack | SP47-T1-SP47-T5 | Run deterministic verification and PR gate. | All focused and workspace checks pass; PR links GH47 and preserves human gates. | commands below |

## Ownership / Threads

- Main implementation lane owns `crates/agent`, `crates/server`, `web/src`, and `specs/GH47`.
- Read-only thread may review contract/security risks only.
- No parallel writer may touch the same files in this issue.

## Verification Commands

```sh
cd web && npm test -- app.test.tsx
cd web && npm run build
cargo fmt --check
cargo check --workspace
cargo test --workspace
git diff --check
```

## Handoff Notes

- JSON API boundary keeps camelCase.
- Agent ctx/output contracts keep stable English machine identifiers.
- Do not auto-apply proposal output.
- Do not execute provider work before run confirmation.
