# Roadmap

## M0: Runner Without Agent

Goal: build a usable local runner before integrating Agent automation.

Deliverables:

- Rust workspace and axum server.
- SQLite migrations.
- React 19 app shell.
- NodeRegistry with static node definitions.
- Mock provider.
- RunService executing a small DAG.
- REST state endpoint and WebSocket event stream.
- Artifact persistence and output strip.

Acceptance:

- A sample workflow can run without an Agent.
- Outputs persist across app restart.

## M1: Agent Proposal Flow

Goal: let Codex create and modify workflow graphs through reviewed proposals.

Deliverables:

- Codex runtime adapter.
- Agent session `ctx/` and `out/` file contract.
- `node_library`, `create_workflow`, and `modify_workflow` skills.
- Proposal validation and preview.
- Apply/dismiss.
- Version history.

Acceptance:

- User prompt creates a pending graph proposal.
- Applying proposal creates a new immutable version.
- Dismissing proposal leaves the graph unchanged.

## M2: Diagnosis, Cost Gate, Sweep

Goal: close the loop around paid API calls and failures.

Deliverables:

- Cost estimation and ledger.
- Agent requested run confirmation.
- `fix_error` and `sweep` skills.
- Retry.
- Selected outputs.

Acceptance:

- Failed provider run can produce a minimal fix proposal.
- Agent requested sweep requires explicit confirmation.
- Sweep produces multiple outputs and a selected recommendation.

## M3: Expansion

Deliverables:

- Claude Code runtime.
- Additional providers.
- Optional ComfyUI provider.
- Manual parameter editing.
- Redo.
- Multi-workspace management.
- Brew/prebuilt binary packaging.
