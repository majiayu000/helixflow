# Initial Issue Backlog

This file mirrors the first GitHub issue queue.

## 1. Scaffold Rust workspace and React 19 app

Create the repository skeleton for the chosen architecture.

Acceptance:

- Cargo workspace with core crates.
- React 19 Vite app under `web/`.
- Basic build commands documented.
- No runtime provider secrets.

## 2. Implement SQLite store and migrations

Create persistence foundations for workspaces, versions, proposals, messages, runs, steps, artifacts, providers, and cost ledger.

Acceptance:

- Migrations can create a fresh database.
- Store tests cover workspace and version creation.
- WAL mode enabled.

## 3. Implement NodeRegistry and mock provider

Create the first runnable non-Agent execution path.

Acceptance:

- Static node definitions load from repo data.
- Mock provider implements provider trait.
- Registry validation rejects unknown node types.

## 4. Implement GraphService proposal and version flow

Support graph validation, diff preview, proposal apply/dismiss, and immutable versions.

Acceptance:

- Proposal ops are schema validated.
- Applying a proposal creates a version.
- Dismissing a proposal leaves graph unchanged.
- Superseded base versions return conflict.

## 5. Implement RunService with WebSocket progress

Execute a simple DAG through mock provider and stream state to the frontend.

Acceptance:

- Manual run creates run and run_step rows.
- Node states stream through WebSocket.
- Artifacts persist.
- Interrupt cancels remaining steps.

## 6. Port prototype shell to React 19 TypeScript

Turn the static prototype into the first real frontend shell.

Acceptance:

- Top bar, chat pane, canvas, run dock, outputs, history, and confirm modal render.
- Frontend state comes from backend `/state`.
- No business truth is derived only in the frontend.

## 7. Add Codex runtime and Agent proposal contract

Integrate the first Agent runtime after the runner works without Agent.

Acceptance:

- Backend creates `ctx/` and reads `out/proposal.json`.
- Agent output is validated before persistence.
- Agent cannot access provider secrets.

## 8. Add cost gate and sweep plan

Add paid-call safety and the first multi-run workflow.

Acceptance:

- Agent requested run emits `run.requested`.
- User confirmation is required before provider invocation.
- Cost ledger records estimates and actuals.
