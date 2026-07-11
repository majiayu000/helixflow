# Helixflow

Helixflow is a local-first AI workflow orchestrator: users describe generation tasks, an Agent edits node graphs through validated version transactions, and the backend executes workflows through model providers with a cost gate.

The name combines "helix" and "flow": workflows move forward while each Agent edit, run, error, and fix forms an iterative loop that tightens the result.

## Current State

This repository currently contains:

- Static interaction prototypes for the original ComfyUI Agent concept.
- Architecture spec drafts v0.1, v0.2, and v0.3.
- A complete workflow-orchestrator spec in [SPEC_WORKFLOW_ORCHESTRATOR.md](SPEC_WORKFLOW_ORCHESTRATOR.md).

The first scaffold now includes a minimal local server and React workbench shell.

## Chosen Direction

The active direction is the workflow orchestrator route:

- Frontend: React 19 + TypeScript + Vite.
- Backend: Rust + axum + tokio + sqlx + SQLite.
- Execution: backend `RunService` executes graph nodes through `ModelGateway` providers.
- Agent: Codex CLI first, producing validated graph transactions and run plans through a sandboxed `ctx/` and `out/` contract.
- ComfyUI: future optional provider, not the v1 execution core.

## MVP Milestones

1. M0: runner without Agent, using mock provider plus one real provider.
2. M1: Agent auto-applied graph transactions, version history, rollback.
3. M2: diagnosis, cost gate, sweep, retry.
4. M3: more providers, optional ComfyUI provider, packaging.

## Development

Install frontend dependencies:

```sh
cd web
npm install
```

Run checks:

```sh
cargo check --workspace
cd web && npm run build
```

## Agent proposals and run confirmation

Validated Agent graph proposals are applied atomically as immutable workflow
versions. The proposal, version, current-workspace pointer, and
`proposal_applied` message commit together; version history remains the rollback
surface. Applying a proposal does not implicitly start a run.

Agent `RunRequest` and seed sweep requests estimate cost before execution.
Set `HELIXFLOW_AGENT_RUN_CONFIRMATION_THRESHOLD_USD` to a finite,
non-negative USD amount:

- estimates at or below the threshold start through the normal confirmed-run
  entrypoint;
- estimates above the threshold stay in `waiting_confirmation` until approved;
- an unset value defaults to `0`, while an invalid value fails the request
  explicitly before a run record is created.

The implementation workspace is:

```text
crates/
  server/
  store/
  graph/
  registry/
  gateway/
  run/
  agent/
web/
e2e/
```

## Repository Policy

- Keep `main` stable.
- Work in focused branches.
- Each PR should map to one issue.
- Do not introduce provider secrets into repo, logs, DB fixtures, or Agent context.
