# Helixflow

Helixflow is a local-first AI workflow orchestrator: users describe generation tasks, an Agent proposes node-graph changes, and the backend executes approved workflows through model providers.

The name combines "helix" and "flow": workflows move forward while each Agent proposal, run, error, and fix forms an iterative loop that tightens the result.

## Current State

This repository currently contains:

- Static interaction prototypes for the original ComfyUI Agent concept.
- Architecture spec drafts v0.1, v0.2, and v0.3.
- A complete workflow-orchestrator spec in [SPEC_WORKFLOW_ORCHESTRATOR.md](SPEC_WORKFLOW_ORCHESTRATOR.md).

There is no runnable app yet.

## Chosen Direction

The active direction is the workflow orchestrator route:

- Frontend: React 19 + TypeScript + Vite.
- Backend: Rust + axum + tokio + sqlx + SQLite.
- Execution: backend `RunService` executes graph nodes through `ModelGateway` providers.
- Agent: Codex CLI first, producing proposals and run plans through a sandboxed `ctx/` and `out/` contract.
- ComfyUI: future optional provider, not the v1 execution core.

## MVP Milestones

1. M0: runner without Agent, using mock provider plus one real provider.
2. M1: Agent proposal flow, version history, apply/dismiss.
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
