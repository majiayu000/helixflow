# AI Workflow Orchestrator Spec

Status: v1 draft  
Date: 2026-06-12  
Target: local-first node workflow orchestration app  
Frontend: React 19 + TypeScript + Vite  
Backend: Rust + axum + tokio + sqlx + SQLite  

## 1. Decision

This spec chooses the "workflow orchestrator" direction.

The app is not a thin ComfyUI client in v1. It is a local web app that lets users create, inspect, modify, and run AI generation workflows. The workflow is represented as a node graph. Each executable node maps to a provider capability exposed through the backend Model Gateway, such as Seedance video generation, image generation, LLM text generation, image analysis, or future local ComfyUI execution.

ComfyUI compatibility is preserved as a future provider integration, not as the v1 execution engine.

## 2. Product Goal

Users should be able to describe an AI generation workflow in natural language, review the generated node graph, apply or reject changes, run the graph with clear cost confirmation, inspect progress per node, and review outputs.

Primary workflow:

1. User describes a desired generation task.
2. Agent proposes a graph change.
3. User reviews graph diff.
4. User applies the proposal.
5. User manually runs the graph or approves an Agent requested run.
6. Backend executes nodes through providers.
7. Frontend shows per-node state, artifacts, errors, cost, and history.

## 3. Non-Goals For V1

- Multi-user account system.
- Cloud-hosted backend.
- Public remote access.
- Model training.
- Full manual drag-and-drop graph editing.
- Parallel multi-provider scheduling optimization.
- Full ComfyUI compatibility as the main execution engine.
- Provider marketplace.
- Team collaboration.

V1 can support selecting nodes, inspecting parameters, reviewing diffs, applying proposals, running workflows, retrying failed runs, and selecting outputs.

## 4. Core Principles

### 4.1 Backend Is Source Of Truth

The frontend never directly calls providers, Agent CLIs, model APIs, or local files. All durable state comes from the backend.

Frontend startup and reconnect use:

```http
GET /api/workspaces/{workspace_id}/state
```

WebSocket events update visible state, but `/state` is the reconciliation source.

### 4.2 Propose Then Apply

Agent never directly mutates a graph. Agent writes a proposal file. Backend validates it, persists it as pending, broadcasts it to the frontend, and waits for user action.

Only `POST /api/proposals/{id}/apply` creates a new version.

### 4.3 Cost Gate

Any Agent-triggered run that can consume paid API quota requires user confirmation before backend invokes providers.

Manual Queue clicks are treated as explicit user approval, but still enforce cost limits.

### 4.4 Agent Does Not Execute

Agent creates or modifies graph structure and run plans. It does not call providers, hold API keys, write database state, or execute model APIs.

### 4.5 Minimal Diff

Agent proposals should use the smallest operation sequence that satisfies the user request or fixes an error.

### 4.6 Traceability

Every graph change creates an immutable version snapshot. Every run is tied to a graph version. Every provider call is tied to a run step and cost ledger entry.

## 5. Runtime Topology

```text
Browser
  React 19 SPA
    |
    | REST /api/*
    | WebSocket /ws?workspace_id=...
    v
Local Backend 127.0.0.1:8787
  axum server
  AgentOrchestrator
  GraphService
  RunService
  ModelGateway
  NodeRegistry
  Storage
    |
    | child process, sandboxed workspace dir
    v
Agent CLI
  Codex CLI or Claude Code
  reads ctx/
  writes out/proposal.json or out/run_plan.json

Backend outbound only:
  ModelGateway -> Provider APIs
  Agent CLI -> its own model API
```

## 6. Technology Stack

### 6.1 Frontend

- React 19.
- TypeScript.
- Vite.
- Zustand for domain stores.
- TanStack Query only for REST request lifecycle, not as the source of domain truth.
- Zod or generated TypeScript types for API payload validation.
- Canvas rendering with DOM nodes + SVG edges, based on the current prototype approach.

### 6.2 Backend

- Rust stable.
- axum for REST and WebSocket.
- tokio for async runtime.
- sqlx with SQLite WAL.
- serde for data model serialization.
- jsonschema or schemars for validating Agent outputs.
- reqwest for provider APIs.
- tower-http for static frontend serving.
- rust-embed or equivalent for embedding frontend build artifacts.

### 6.3 Local Data

Default data directory:

```text
~/.ai-workflow-orchestrator/
  config.toml
  workbench.db
  workspaces/
    {workspace_id}/
      graphs/
      proposals/
      uploads/
      outputs/
      agent_sessions/
```

The existing project can keep the visible product name, but the runtime/data directory should avoid claiming ComfyUI-only scope if this route is chosen.

## 7. Backend Modules

### 7.1 `crates/server`

Responsibilities:

- Start axum HTTP server.
- Serve React static assets.
- Register REST routes.
- Register WebSocket endpoint.
- Inject shared app state.
- Convert domain errors into API errors.
- Enforce local binding by default.

### 7.2 `crates/store`

Responsibilities:

- SQLite connection pool.
- Migrations.
- Metadata tables.
- File storage helpers.
- Safe path joining and prefix checks.
- Content-addressed upload dedupe.

### 7.3 `crates/graph`

Responsibilities:

- Canonical graph model.
- Graph validation.
- Proposal operation validation.
- Proposal application.
- Version creation.
- Diff summary generation.
- Preview graph generation.
- Layout calculation.
- Compile graph to executable plan.

### 7.4 `crates/registry`

Responsibilities:

- Own NodeRegistry.
- Load built-in node definitions.
- Expose node schemas to GraphService, RunService, frontend, and Agent context.
- Bind node types to provider capabilities.
- Export `ctx/node_defs/` for Agent sessions.

### 7.5 `crates/gateway`

Responsibilities:

- Own provider trait.
- Hold provider config and secret references.
- Invoke providers.
- Poll async provider tasks.
- Normalize provider outputs.
- Estimate and record costs.
- Enforce daily cost limit.
- Handle retry, timeout, and rate limit policy.
- Export `ctx/catalog.json`.

### 7.6 `crates/run`

Responsibilities:

- Compile version graph into an execution plan through GraphService.
- Execute DAG by topological order.
- Manage run status.
- Manage run steps.
- Stream node and run progress events.
- Pass outputs between nodes.
- Persist artifacts.
- Cancel active and pending nodes.
- Retry failed runs.
- Support sweep plans.
- Trigger Agent diagnosis on failure when appropriate.

### 7.7 `crates/agent`

Responsibilities:

- Manage one Agent session per workspace.
- Route user messages to the right runtime.
- Build `ctx/` directory per turn.
- Select and inject skills.
- Start Codex or Claude Code runtime.
- Parse runtime JSON event stream.
- Convert runtime progress to `agent.status`.
- Validate `out/proposal.json`.
- Validate `out/run_plan.json`.
- Store Agent transcripts without secrets.

## 8. Canonical Graph Model

The graph is a provider-neutral workflow representation.

```json
{
  "schema_version": 1,
  "nodes": {
    "n1": {
      "type": "text_to_video.seedance2",
      "title": "Seedance 2 Video",
      "params": {
        "text": "A cinematic product ad...",
        "duration_sec": 5,
        "aspect_ratio": "9:16",
        "model": "seedance-2.0-pro"
      },
      "pos": [520, 86]
    }
  },
  "edges": [
    {
      "from": ["n0", "image"],
      "to": ["n1", "reference_image"],
      "type": "IMAGE"
    }
  ]
}
```

### 8.1 Node ID

- Stable within a graph version.
- Generated by backend when user manually creates nodes.
- May be proposed by Agent, but backend validates uniqueness.

### 8.2 Node Type

Must exist in NodeRegistry.

Examples:

- `input.image`
- `input.text`
- `analysis.image_caption`
- `llm.prompt_writer`
- `image.generate`
- `video.seedance2.text_to_video`
- `video.seedance2.image_to_video`
- `utility.select_best`
- `output.save`

Avoid hard-coding ComfyUI class names in v1 if the chosen route is Model Gateway.

### 8.3 Edge Type

Allowed primitive port types:

- `TEXT`
- `IMAGE`
- `VIDEO`
- `AUDIO`
- `MASK`
- `JSON`
- `MODEL`
- `CONDITIONING`
- `LATENT`

The first six are v1 execution types. `MODEL`, `CONDITIONING`, and `LATENT` are allowed for UI familiarity and future ComfyUI provider support, but they should not be required for Seedance-only MVP.

## 9. NodeRegistry

NodeRegistry is the single source of truth for available node types.

### 9.1 Node Definition

```json
{
  "type": "video.seedance2.image_to_video",
  "title": "Seedance 2 Image to Video",
  "category": "video",
  "provider": "seedance2",
  "capability": "image_to_video",
  "description": "Generate a video from an input image and text prompt.",
  "inputs": [
    {
      "name": "reference_image",
      "type": "IMAGE",
      "required": true
    },
    {
      "name": "prompt",
      "type": "TEXT",
      "required": true
    }
  ],
  "outputs": [
    {
      "name": "video",
      "type": "VIDEO"
    }
  ],
  "params_schema": {
    "type": "object",
    "required": ["model", "duration_sec", "aspect_ratio"],
    "properties": {
      "model": {
        "type": "string",
        "enum_from_catalog": "seedance2.models"
      },
      "duration_sec": {
        "type": "integer",
        "minimum": 3,
        "maximum": 10
      },
      "aspect_ratio": {
        "type": "string",
        "enum": ["9:16", "16:9", "1:1"]
      },
      "seed": {
        "type": "integer"
      }
    }
  },
  "estimated_cost": {
    "unit": "call",
    "catalog_key": "seedance2.image_to_video"
  }
}
```

### 9.2 Registry Rules

- Node types are versioned.
- Provider binding can change without changing the canonical graph if the capability remains compatible.
- Removed provider models are reflected in catalog refresh, not silently accepted.
- Unknown node type is a hard validation error.
- Unknown optional param can be warning only if node type allows provider-specific passthrough.

## 10. ModelGateway

### 10.1 Provider Trait

Rust trait shape:

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    async fn health(&self) -> ProviderHealth;
    async fn catalog(&self) -> Result<ProviderCatalog, ProviderError>;
    async fn estimate(&self, req: ProviderRequest) -> Result<CostEstimate, ProviderError>;
    async fn invoke(&self, req: ProviderRequest) -> Result<ProviderResult, ProviderError>;
    async fn cancel(&self, handle: ProviderTaskHandle) -> Result<(), ProviderError>;
}
```

### 10.2 Provider Request

```json
{
  "provider": "seedance2",
  "capability": "image_to_video",
  "node_id": "n4",
  "run_id": "run_123",
  "inputs": {
    "reference_image": {
      "artifact_id": "art_1",
      "storage_uri": "workspace://uploads/product.png"
    }
  },
  "params": {
    "model": "seedance-2.0-pro",
    "prompt": "A cinematic product ad...",
    "duration_sec": 5,
    "aspect_ratio": "9:16"
  }
}
```

### 10.3 Provider Result

```json
{
  "outputs": {
    "video": {
      "kind": "video",
      "mime": "video/mp4",
      "storage_uri": "workspace://outputs/run_123/n4/video.mp4",
      "width": 1080,
      "height": 1920,
      "duration_ms": 5000,
      "meta": {
        "provider_task_id": "abc",
        "model": "seedance-2.0-pro"
      }
    }
  },
  "cost": {
    "currency": "USD",
    "estimated": false,
    "amount": 0.24
  }
}
```

### 10.4 Provider Rules

- Secrets are loaded from environment variables referenced by `config.toml`.
- Secrets never enter logs, DB rows, Agent session directories, or frontend responses.
- Provider outbound URLs must be allowlisted per provider config.
- Provider errors are normalized.
- Provider calls are tied to `run_step_id`.
- Async tasks must be pollable or timeout with a clear error.

## 11. GraphService

### 11.1 Validation

Graph validation checks:

- Node type exists.
- Node params match schema.
- Required inputs are connected or provided by params.
- Edge endpoints exist.
- Edge port names exist.
- Edge types match.
- Graph is acyclic.
- Output node exists for run-ready graphs.

### 11.2 Compile To Execution Plan

The compiled execution plan is backend-internal and versioned.

```json
{
  "schema_version": 1,
  "version_id": "ver_3",
  "steps": [
    {
      "node_id": "n1",
      "node_type": "input.image",
      "provider": "builtin",
      "capability": "load_upload",
      "inputs": {},
      "params": {
        "upload_id": "upl_1"
      }
    },
    {
      "node_id": "n2",
      "node_type": "video.seedance2.image_to_video",
      "provider": "seedance2",
      "capability": "image_to_video",
      "inputs": {
        "reference_image": ["n1", "image"]
      },
      "params": {
        "model": "seedance-2.0-pro",
        "prompt": "..."
      }
    }
  ]
}
```

The frontend never needs this full plan unless debugging is enabled.

## 12. Proposal Model

### 12.1 Proposal Object

```json
{
  "id": "prop_12",
  "workspace_id": "ws_1",
  "base_version_id": "ver_2",
  "kind": "create",
  "title": "Product ad video workflow",
  "summary": "Create an image-to-video workflow using Seedance 2.",
  "ops": [],
  "diff_summary": [],
  "preview_graph": {},
  "state": "pending",
  "message_id": "msg_9",
  "created_at": "2026-06-12T00:00:00Z"
}
```

### 12.2 Operation Types

Allowed ops:

- `add_node`
- `remove_node`
- `set_param`
- `add_edge`
- `remove_edge`
- `move_node`

No separate `set_prompt` op. Prompt editing is `set_param` on a node text field.

### 12.3 `set_param`

```json
{
  "op": "set_param",
  "id": "n2",
  "key": "prompt",
  "prev": "Old prompt",
  "value": "New prompt"
}
```

Rules:

- Agent may omit `prev`.
- Backend fills `prev` during normalization.
- If Agent supplies `prev` and it does not match the base version, mark proposal `superseded`.
- Large text diffs are rendered with old/new side-by-side in the frontend.

### 12.4 Proposal State Machine

```text
drafting -> pending -> applied
                    -> dismissed
                    -> superseded
                    -> invalid
```

Only one pending proposal per workspace in v1.

## 13. RunService

### 13.1 Run State Machine

```text
queued -> estimating -> waiting_confirmation -> running -> succeeded
                                                   -> failed
                                                   -> interrupted
                                                   -> cancelled
```

Manual runs can skip `waiting_confirmation` unless cost limit requires confirmation.

Agent requested runs always enter `waiting_confirmation`.

### 13.2 Run Step State Machine

```text
pending -> active -> succeeded
                  -> failed
                  -> cancelled
```

### 13.3 Execution Rules

- Compile graph from the exact version_id recorded on the run.
- Do not execute if graph has pending proposal.
- Do not mutate graph during run.
- Execute in topological order.
- Store each node output in memory for downstream nodes and persist final artifacts.
- For long provider tasks, emit phase events.
- If a required upstream node fails, mark downstream nodes cancelled.
- On interrupt, cancel active provider task where supported and skip remaining steps.

### 13.4 Sweep Runs

A sweep is a run group.

```json
{
  "kind": "sweep",
  "runs": 4,
  "overrides": [
    { "node_id": "n2", "params": { "seed": 1001 } },
    { "node_id": "n2", "params": { "seed": 1002 } }
  ],
  "selection_policy": {
    "type": "agent_recommend"
  }
}
```

V1 sweep runs are serial. Parallel sweep execution is M3+.

## 14. Agent Runtime

### 14.1 Runtime Trait

```rust
#[async_trait]
pub trait AgentRuntime: Send + Sync {
    fn id(&self) -> &'static str;
    async fn start(&self, session: AgentSession) -> Result<RuntimeHandle, RuntimeError>;
    async fn send(&self, handle: &RuntimeHandle, turn: AgentTurn) -> Result<(), RuntimeError>;
    async fn next_event(&self, handle: &RuntimeHandle) -> Option<RuntimeEvent>;
    async fn cancel(&self, handle: &RuntimeHandle) -> Result<(), RuntimeError>;
}
```

### 14.2 Runtime Implementations

MVP:

- `CodexRuntime`

Later:

- `ClaudeCodeRuntime`

### 14.3 Session Directory

```text
agent_sessions/
  {session_id}/
    ctx/
      graph.json
      node_defs/
      catalog.json
      run_error.json
      instructions.md
      skills/
        node_library.md
        create_workflow.md
    out/
      proposal.json
      run_plan.json
      final_message.md
    transcript.jsonl
```

### 14.4 Context Rules

- `ctx/` is generated by backend.
- `out/` is Agent-writable.
- CLI sandbox root is the session directory.
- Shell execution is disabled when runtime supports it.
- No user filesystem paths outside workspace.
- No provider secrets.
- No raw API keys.

## 15. Skills

Skills are versioned Markdown instruction files used by the Agent.

Required v1 skills:

```text
skills/
  node_library.md
  create_workflow.md
  modify_workflow.md
  fix_error.md
  sweep.md
```

### 15.1 Skill Selection

`AgentOrchestrator` routes user intent:

- Empty/initial graph + generation request -> `create_workflow.md`.
- Existing graph + edit request -> `modify_workflow.md`.
- Failed run diagnosis -> `fix_error.md`.
- Multi-seed or parameter experiment -> `sweep.md`.
- Always inject `node_library.md`.

### 15.2 Skill Output Requirements

Every skill must instruct the Agent to produce exactly one of:

- `out/proposal.json`
- `out/run_plan.json`
- `out/final_message.md`

If no graph change is needed, Agent writes `final_message.md`.

## 16. REST API

All responses use JSON unless file download is stated.

### 16.1 Workspaces

```http
GET /api/workspaces
POST /api/workspaces
GET /api/workspaces/{id}/state
PATCH /api/workspaces/{id}
DELETE /api/workspaces/{id}
```

V1 may hide delete in UI but backend should support safe deletion or archive.

### 16.2 Messages

```http
POST /api/workspaces/{id}/messages
```

Request:

```json
{
  "text": "Create a short product ad video workflow",
  "attachments": ["upl_1"]
}
```

Response:

```json
{
  "message_id": "msg_1"
}
```

### 16.3 Uploads

```http
POST /api/workspaces/{id}/uploads
GET /api/uploads/{id}
```

Upload uses multipart.

### 16.4 Proposals

```http
POST /api/proposals/{id}/apply
POST /api/proposals/{id}/dismiss
```

Apply response:

```json
{
  "version_id": "ver_4",
  "cur_idx": 4
}
```

### 16.5 Versions

```http
POST /api/workspaces/{id}/undo
POST /api/versions/{id}/restore
GET /api/versions/{id}/export
```

Export returns canonical workflow JSON, not provider-specific request payloads.

### 16.6 Runs

```http
POST /api/workspaces/{id}/runs
POST /api/runs/confirm
POST /api/runs/{id}/interrupt
POST /api/runs/{id}/retry
GET /api/runs/{id}
GET /api/runs/{id}/events
GET /api/runs/{id}/artifacts
```

Manual run request:

```json
{
  "version_id": "ver_4",
  "overrides": {}
}
```

Confirm request:

```json
{
  "plan_id": "plan_1",
  "approve": true
}
```

### 16.7 Outputs

```http
POST /api/outputs/{id}/select
GET /api/artifacts/{id}
GET /api/artifacts/{id}/thumbnail
```

### 16.8 Registry And Providers

```http
GET /api/node-defs
GET /api/node-defs/{type}
GET /api/providers
POST /api/providers/{id}/test
GET /api/conn
```

## 17. WebSocket API

Endpoint:

```text
/ws?workspace_id={workspace_id}
```

Server-to-client event envelope:

```json
{
  "seq": 123,
  "workspace_id": "ws_1",
  "server_time": "2026-06-12T00:00:00Z",
  "ev": "run.progress",
  "data": {}
}
```

### 17.1 Events

Required events:

- `agent.status`
- `agent.status.end`
- `agent.message`
- `proposal.created`
- `proposal.resolved`
- `version.committed`
- `run.requested`
- `run.queued`
- `run.estimate.updated`
- `run.started`
- `run.progress`
- `node.state`
- `artifact.created`
- `outputs.updated`
- `run.failed`
- `run.interrupted`
- `run.done`
- `conn.status`

### 17.2 Reconnect Rule

On WebSocket reconnect:

1. Frontend calls `GET /api/workspaces/{id}/state`.
2. Frontend replaces server-owned state.
3. Frontend reconnects WebSocket.
4. Events older than the latest state snapshot sequence are ignored.

## 18. Database Schema

### 18.1 Core Tables

```sql
workspaces(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  cur_version_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

versions(
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  idx INTEGER NOT NULL,
  label TEXT NOT NULL,
  source TEXT NOT NULL,
  graph_path TEXT NOT NULL,
  graph_hash TEXT NOT NULL,
  parent_id TEXT,
  created_at TEXT NOT NULL
);

proposals(
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  base_version_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  title TEXT NOT NULL,
  summary TEXT NOT NULL,
  ops_path TEXT NOT NULL,
  preview_graph_path TEXT,
  state TEXT NOT NULL,
  result_version_id TEXT,
  message_id TEXT,
  created_at TEXT NOT NULL,
  resolved_at TEXT
);

messages(
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  role TEXT NOT NULL,
  text TEXT,
  kind TEXT NOT NULL,
  ref_id TEXT,
  attachment_ids_json TEXT,
  created_at TEXT NOT NULL
);
```

### 18.2 Run Tables

```sql
runs(
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  version_id TEXT NOT NULL,
  group_id TEXT,
  label TEXT NOT NULL,
  trigger TEXT NOT NULL,
  plan_json TEXT,
  estimate_json TEXT,
  status TEXT NOT NULL,
  error_json TEXT,
  started_at TEXT,
  ended_at TEXT,
  created_at TEXT NOT NULL
);

run_steps(
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  node_id TEXT NOT NULL,
  node_type TEXT NOT NULL,
  provider TEXT,
  state TEXT NOT NULL,
  progress REAL,
  cost_estimate_json TEXT,
  cost_actual_json TEXT,
  error_json TEXT,
  started_at TEXT,
  ended_at TEXT
);

run_events(
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  ev TEXT NOT NULL,
  data_json TEXT NOT NULL,
  created_at TEXT NOT NULL
);
```

### 18.3 Artifact Tables

```sql
uploads(
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  filename TEXT NOT NULL,
  file_path TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  mime TEXT,
  created_at TEXT NOT NULL
);

artifacts(
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  run_id TEXT,
  run_step_id TEXT,
  node_id TEXT,
  kind TEXT NOT NULL,
  storage_uri TEXT NOT NULL,
  sha256 TEXT,
  mime TEXT,
  width INTEGER,
  height INTEGER,
  duration_ms INTEGER,
  selected INTEGER NOT NULL DEFAULT 0,
  meta_json TEXT,
  created_at TEXT NOT NULL
);
```

### 18.4 Provider And Cost Tables

```sql
providers(
  id TEXT PRIMARY KEY,
  enabled INTEGER NOT NULL,
  status TEXT NOT NULL,
  catalog_hash TEXT,
  catalog_path TEXT,
  last_checked_at TEXT
);

cost_ledger(
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  run_id TEXT,
  run_step_id TEXT,
  provider TEXT NOT NULL,
  amount REAL NOT NULL,
  currency TEXT NOT NULL,
  estimated INTEGER NOT NULL,
  created_at TEXT NOT NULL
);
```

## 19. Frontend Stores

### 19.1 `useGraphStore`

Owns:

- `versions`
- `curVersionId`
- `currentGraph`
- `pendingProposal`
- `selectedNodeId`

Server events:

- `proposal.created`
- `proposal.resolved`
- `version.committed`

### 19.2 `useChatStore`

Owns:

- `messages`
- `agentStatus`
- `busy`

Server events:

- `agent.status`
- `agent.status.end`
- `agent.message`

### 19.3 `useRunStore`

Owns:

- `activeRun`
- `runs`
- `runSteps`
- `outputs`
- `confirmPlan`

Server events:

- `run.requested`
- `run.queued`
- `run.started`
- `run.progress`
- `node.state`
- `artifact.created`
- `outputs.updated`
- `run.failed`
- `run.interrupted`
- `run.done`

### 19.4 `useConnStore`

Owns:

- gateway status
- provider statuses
- node registry version/count
- agent runtime status

Server event:

- `conn.status`

## 20. Security

### 20.1 Network

- Bind backend to `127.0.0.1` by default.
- Do not expose public server mode in v1.
- Provider outbound endpoints are allowlisted.
- No user-provided arbitrary URLs for provider calls unless explicitly enabled and guarded.

### 20.2 Secrets

- Provider keys are environment variables referenced by config.
- Secrets never enter SQLite.
- Secrets never enter logs.
- Secrets never enter Agent session directory.
- Secrets never enter frontend payloads.

### 20.3 Agent Sandbox

- Agent working directory is restricted to session dir.
- Shell execution disabled when possible.
- Agent writes only `out/`.
- Backend validates every Agent output as untrusted input.

### 20.4 Path Safety

- All workspace file paths are normalized.
- All paths must remain inside workspace directory.
- No symlink traversal.
- Uploaded filenames are sanitized and stored under generated IDs.

### 20.5 Cost Safety

- Configurable daily limit.
- Configurable per-run limit.
- Agent requested runs require confirmation.
- Manual runs display estimate when available.
- Provider actual costs are logged.

## 21. Error Model

API errors:

```json
{
  "error": {
    "code": "proposal_superseded",
    "message": "The graph changed before this proposal could be applied.",
    "details": {}
  }
}
```

Common codes:

- `validation_failed`
- `proposal_superseded`
- `pending_proposal_exists`
- `graph_not_runnable`
- `provider_unavailable`
- `provider_rate_limited`
- `provider_task_timeout`
- `cost_limit_exceeded`
- `agent_runtime_unavailable`
- `artifact_not_found`
- `path_forbidden`

## 22. Observability

Required:

- Structured JSON logs.
- Trace ID for every user message, proposal, run, and provider call.
- Run event timeline.
- Provider latency and error counts.
- Cost ledger.
- Agent transcript without secrets.

Optional later:

- Local diagnostics export bundle.
- Anonymous crash reporting, opt-in only.

## 23. Testing Strategy

### 23.1 Unit Tests

- Graph validation.
- Proposal operation application.
- Diff summary generation.
- NodeRegistry schema validation.
- Provider request normalization.
- Cost estimate math.
- Path safety.

### 23.2 Integration Tests

- REST state lifecycle.
- WebSocket event ordering.
- Proposal apply -> version committed.
- Run execution with mock provider.
- Failed node -> Agent fix proposal with mock runtime.
- Upload -> artifact flow.

### 23.3 E2E Tests

Use:

- Mock provider.
- Mock Agent runtime.
- Playwright frontend test.

Critical v1 flows:

1. Create workflow by message.
2. Apply proposal.
3. Run workflow.
4. See per-node progress.
5. See output artifact.
6. Failed provider call triggers error card.
7. Agent proposes minimal fix.
8. Apply fix and rerun.
9. Sweep run creates multiple outputs and selects one.

## 24. MVP Milestones

### M0: Runner Without Agent

Goal: usable workflow runner.

Scope:

- Rust server.
- SQLite store.
- React shell.
- NodeRegistry with 5-8 built-in nodes.
- Mock provider and one real provider.
- Manual import/export workflow JSON.
- Manual Queue.
- Per-node progress.
- Artifact output strip.
- Interrupt.

Acceptance:

- User can run a sample workflow without Agent.
- Outputs persist and reload after app restart.

### M1: Agent Proposal Flow

Goal: Agent can create and modify graphs, but not execute.

Scope:

- CodexRuntime.
- Session ctx/out contract.
- Skills: node_library, create_workflow, modify_workflow.
- Proposal validation.
- Proposal preview.
- Apply/dismiss.
- Version history.

Acceptance:

- User prompt creates a pending graph proposal.
- Applying proposal creates a new version.
- Dismissing proposal leaves current graph unchanged.

### M2: Diagnosis, Cost Gate, Sweep

Goal: complete closed loop.

Scope:

- fix_error skill.
- sweep skill.
- run_plan validation.
- run confirmation with estimated cost.
- Cost ledger.
- Failure diagnosis.
- Retry.

Acceptance:

- Failed provider run triggers an Agent fix proposal.
- Agent requested sweep requires confirmation.
- Sweep produces multiple outputs and selected recommendation.

### M3: Expansion

Scope:

- Claude Code runtime.
- More providers.
- Optional ComfyUI provider.
- Manual parameter editing.
- Redo.
- Multi-workspace management.
- Packaging through brew/prebuilt binaries.

## 25. Open Decisions

These must be decided before implementation starts:

1. Product name: keep "ComfyUI Agent" or rename to avoid mismatch.
2. First real provider: Seedance 2 only, or include image provider too.
3. Provider API credential source and config format.
4. Whether v1 includes manual node parameter edits.
5. Whether output video thumbnails are generated locally or by provider.
6. Exact Codex CLI JSON mode contract for current installed version.
7. Whether Agent transcripts are visible in UI in v1 or v1.1.

## 26. Recommended Immediate Implementation Plan

1. Create Cargo workspace and React app skeleton.
2. Implement SQLite migrations.
3. Implement NodeRegistry with static JSON definitions.
4. Implement mock provider.
5. Implement RunService over mock provider.
6. Implement REST `/state`, `/runs`, `/uploads`, `/artifacts`.
7. Implement WebSocket event broadcaster.
8. Port prototype UI to React 19 TypeScript.
9. Wire frontend to backend state.
10. Add CodexRuntime only after runner works without Agent.

## 27. Compatibility Note

If the project goal returns to pure ComfyUI Agent, use the v2 architecture instead:

- Replace ModelGateway with ComfyAdapter.
- Compile graph to ComfyUI API workflow JSON.
- Use ComfyUI `/prompt`, `/ws`, `/history`, `/view`, `/upload/image`.
- Keep Runtime/Skill/Proposal/Version concepts from this spec.

Do not mix both execution strategies in M0. Choose one execution core first.
