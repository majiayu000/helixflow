# Canvas Agent Research

Status: research notes from live Chrome testing and repository inspection on 2026-06-30.

Scope:

- Explain what this repository currently implements.
- Record the observed Higgsfield Canvas frontend data flow and backend split.
- Compare that model with Krea Nodes and Lovart ChatCanvas public product evidence.
- Map the gaps between Helixflow and a production canvas-agent architecture.

This document intentionally separates observed facts from inferred backend shape. Anything not directly verified is marked as not verified.

## Evidence Levels

| Label | Meaning |
| --- | --- |
| Live Chrome | Observed in the user's logged-in Chrome session with the app running. |
| Runtime probe | Captured by a temporary in-page probe that wrapped `fetch`, `XMLHttpRequest.send`, and `WebSocket.prototype.send`; sensitive values were redacted. |
| Static JS | Found in downloaded production JavaScript bundles. This proves frontend code paths exist, not that a specific path was triggered during the session. |
| Public source | Vendor blog/docs/news page. Useful for product model, not packet-level backend proof. |
| Local code | Verified from this repository's source. |
| Inference | Backend shape inferred from observed frontend traffic and endpoint boundaries. |

## Local Artifacts

Sensitive raw probe captures were deleted after redaction. The remaining local artifacts are:

- `/tmp/higgsfield-probe-events-2-sanitized.json`: sanitized event summary.
- `/tmp/hf_probe_install.js`: probe source used in Chrome DevTools.
- `/tmp/higgsfield-bundles-1782803792/page-1bf1c3c938686012.js`: Higgsfield Canvas page bundle.

The sanitized probe preserves endpoint names, status codes, message types, op shapes, and redacted identifiers. It does not preserve auth tickets, session IDs, job IDs, or wallet/subscription balances.

## What Helixflow Is Today

Helixflow is currently a local-first AI workflow orchestrator. The README describes the product direction as: users describe generation tasks, an Agent proposes node-graph changes, and the backend executes approved workflows through model providers.

Local code evidence:

- `README.md`: local-first AI workflow orchestrator; React frontend, Rust backend, backend `RunService`, Codex CLI proposal flow, ComfyUI as future optional provider.
- `SPEC_WORKFLOW_ORCHESTRATOR.md`: backend is source of truth; frontend reconciles with `GET /api/workspaces/{workspace_id}/state`; WebSocket only updates visible state.
- `SPEC_WORKFLOW_ORCHESTRATOR.md`: Agent never directly mutates a graph; it writes a proposal and the backend validates, persists, and waits for user action.
- `crates/server/src/main.rs`: routes include workspaces, state, messages, runs, proposal apply/dismiss, run confirmation, and `/ws`.
- `web/src/api.ts`: frontend uses REST for state/actions and `ws://.../ws?workspace_id=...` for events.

Current architecture:

```text
Browser React SPA
  -> REST /api/workspaces/*
  -> WebSocket /ws?workspace_id=...
Local Rust backend
  -> Workbench
  -> Agent session contract ctx/ + out/
  -> GraphService proposal validation
  -> RunService execution
  -> Provider abstraction
SQLite/local files
```

This is not yet a Higgsfield-style collaborative infinite canvas system. Helixflow has a graph/proposal/run foundation, but it does not yet have a dedicated canvas-worker, ticket-based canvas socket, durable canvas op log, awareness channel, or job-result backfill endpoint.

## Higgsfield Live Chrome Findings

Tested surface:

- `/canvas`: canvas list page.
- `/canvas/{canvasId}`: canvas editor route.
- Existing project title observed in the app UI: `vtuber`.
- Canvas ID and account-specific values are redacted in this document.

Observed editor UI:

- Dark infinite canvas grid.
- Media/video/generation cards on the canvas.
- Left-side controls including undo, redo, minimap, and zoom.
- Top-right controls including Team Chat and Share.
- Bottom toolbar including Select, Pan, Draw, Sticky Note, Shape, Text, Arrow, Sticker, Comment, Folder, Add node, and Open Higgsie.

Add node menu items observed:

- Prompt.
- Image Generator.
- Video Generator.
- Voice Generator.
- LLM Assistant.
- New Folder.
- Upload.
- Assets.
- Voiceover.
- Change Voice.
- Translate.
- Text.
- Sticky Note.

Selecting an existing Image Generation node opened a details panel showing generation settings. The observed panel included model selection, aspect ratio, resolution, batch count, reference images, and paid actions such as Regenerate and Run pipeline. No paid generation action was clicked.

## Higgsfield Canvas Transport

Live Chrome/runtime-probe evidence confirms this sequence:

```http
GET https://canvas-worker.higgsfield.ai/api/flow/{canvasId}/auth
```

The response body had this shape:

```json
{
  "ticket": "<redacted>"
}
```

The editor then opened a WebSocket:

```text
wss://canvas-worker.higgsfield.ai/api/flow/connect/{canvasId}?ticket=<redacted>&format=json
```

Initial sync message:

```json
{
  "type": "op",
  "kind": "sync",
  "lastSeq": 611,
  "pendingOpIds": []
}
```

Awareness messages were sent over the same canvas-worker socket:

```json
{
  "type": "awareness",
  "has_cursor": true,
  "has_drag": false,
  "has_drawing": false,
  "clientId": "<redacted>"
}
```

Observed meaning:

- `op` messages are for durable canvas sync.
- `awareness` messages are for transient collaboration state such as cursor, drag, and drawing.
- `lastSeq` gives the client's last known durable sequence.
- `pendingOpIds` carries local unacknowledged op IDs during reconnect/sync.

This supports a backend split where `canvas-worker.higgsfield.ai` owns canvas authorization, connection, op sequencing, and live collaboration transport.

## Higgsfield Sticky Note Patch

Live operation performed:

- Clicked Sticky Note from the canvas toolbar.
- Clicked an empty canvas area.
- A visible empty sticky note appeared on the canvas.

The patch was captured as a WebSocket send. First, the client sent sync with pending ops:

```json
{
  "type": "op",
  "kind": "sync",
  "lastSeq": 611,
  "pendingOpIds": [
    "<op-id-1>",
    "<op-id-2>",
    "<op-id-3>",
    "<op-id-4>",
    "<op-id-5>",
    "<op-id-6>"
  ]
}
```

Then the client sent an op payload summarized as:

```json
{
  "type": "op",
  "kind": "op",
  "top_ops": 6,
  "top_types": {
    "node:add": 1,
    "batch": 5
  }
}
```

The `node:add` summary:

```json
{
  "node_type": "stickyNote",
  "position": {
    "x": -501.22557878476226,
    "y": 1243.5707500148005
  },
  "data_keys": [
    "text",
    "fontSize",
    "color",
    "authorUsername",
    "input_type",
    "input"
  ],
  "data_summary": {
    "text": "",
    "fontSize": 14,
    "color": "#fef3c7",
    "input_type": "text"
  },
  "style": {
    "width": 200,
    "height": 200
  },
  "zIndex": 15
}
```

The five batch ops each contained three `node:prop` operations. The updated keys were:

- `data.text`.
- `data.richText`.
- `data.input.text`.

Direct conclusion:

- Higgsfield does not save the whole canvas after this action.
- A canvas edit becomes incremental operations over WebSocket.
- Node creation and property initialization are separate patch operations.
- The client has an optimistic outbox model, visible from `pendingOpIds`.

Unverified in this session:

- Exact server acknowledgement frame shape.
- Whether the server accepts idempotent replay by `opId`.
- Conflict resolution strategy if two clients edit the same node field.

## Higgsfield Selection And Detail Fetch

Selecting an existing Image Generation node did not produce a durable canvas op in the captured events. It did trigger a generation/asset detail fetch:

```http
GET https://fnf.higgsfield.ai/assets/{assetId}/detail
```

Sanitized response summary:

```json
{
  "job_set_type": "nano_banana_flash",
  "job_set_id": "<redacted>",
  "params_keys": [
    "aspect_ratio",
    "batch_size",
    "height",
    "medias",
    "prompt",
    "reference_elements",
    "resolution",
    "width"
  ],
  "board_ids_count": 0
}
```

Observed meaning:

- Canvas selection appears to be local UI state unless a panel needs remote detail data.
- Generation node details are not fetched from `canvas-worker.higgsfield.ai`.
- Generation/job/asset data comes from `fnf.higgsfield.ai`.

## Higgsfield Job And Generation Backend Split

Runtime-probe evidence:

```http
GET https://fnf.higgsfield.ai/assets/{assetId}/detail
GET https://fnf.higgsfield.ai/workspaces/wallet
```

Static JS evidence found job and asset endpoints including:

- `GET /job-sets/{id}`.
- `POST /job-sets/{id}/hide`.
- `DELETE /jobs/{id}`.
- `POST /jobs/{id}/view`.
- `POST /jobs/{id}/viewed`.
- `POST /jobs/{id}/track`.
- `GET /jobs/accessible`.
- `POST /jobs/v2/{jobType}` variants.
- `GET /input-images/{id}?result_type=web_optimized|raw`.
- `GET /input-videos/{id}`.

Static JS evidence from the Canvas bundle also found canvas-worker route builders:

- Base host: `https://canvas-worker.higgsfield.ai`.
- API prefix: `/api/flow`.
- Auth: `/api/flow/{id}/auth`.
- Preview: `/api/flow/{id}/preview`.
- WebSocket: `/api/flow/connect/{id}` with `ticket` and `format`.
- Flow: `/api/flow/{id}`.
- Run: `/api/flow/{id}/run`.
- Cancel node: `/api/flow/{id}/nodes/{nodeId}/cancel`.
- From job: `/api/flow/{id}/nodes/from-job`.
- Duplicate/use template/node types/canvases/templates endpoints.

Direct conclusion:

- Canvas state and collaboration live under `canvas-worker.higgsfield.ai`.
- Generation/job/asset/wallet data live under `fnf.higgsfield.ai` and related FNF API hosts.
- The static `nodes/from-job` route strongly indicates a backfill path from generation results into canvas nodes.

Boundary:

- I did not click Run pipeline or Regenerate. Therefore I did not capture a live job creation request or a live `nodes/from-job` response.
- The job/result backfill path is supported by static frontend code and endpoint naming, but the exact runtime payload was not captured.

## Inferred Higgsfield Backend Design

The following is an inference from live traffic and static frontend code, not a direct server-side source read.

```text
Canvas List API
  -> returns canvases/projects for /canvas

Canvas Worker
  -> issues short-lived auth ticket
  -> accepts WS connection per canvas
  -> receives sync(lastSeq, pendingOpIds)
  -> receives durable op batches
  -> broadcasts durable ops to connected clients
  -> receives/broadcasts awareness without committing it to the durable graph
  -> exposes node-level endpoints such as cancel and from-job

Generation/FNF Backend
  -> stores job sets, jobs, assets, wallet/cost state
  -> runs model jobs
  -> exposes asset detail and job tracking
  -> returns job outputs

Backfill Path
  -> generation result becomes an asset/job record
  -> canvas frontend or backend calls nodes/from-job
  -> canvas node is created or updated with the job output reference
```

Probable entity boundaries:

| Entity | Evidence | Likely owner |
| --- | --- | --- |
| Canvas/flow | `/api/flow/{id}`, `/api/flow/connect/{id}` | Canvas worker |
| Auth ticket | `/api/flow/{id}/auth` | Canvas worker |
| Durable canvas op | `type=op`, `kind=op`, `node:add`, `node:prop` | Canvas worker |
| Awareness state | `type=awareness` | Canvas worker memory/pubsub |
| Job set | `job_set_type`, `/job-sets/{id}` | FNF/job backend |
| Job | `/jobs/{id}`, `/jobs/v2/{jobType}` | FNF/job backend |
| Asset | `/assets/{assetId}/detail`, `/input-images/{id}` | FNF/asset backend |
| Wallet/cost | `/workspaces/wallet` | FNF/account backend |
| Canvas result node | `/nodes/from-job` | Bridge from job backend to canvas worker |

## Krea Nodes And Node Agent

Public source evidence:

- Krea's Node Agent page says the agent reads the canvas, plans a pipeline, wires nodes, validates graph parameters/connections, shows cost per node, and does not run until approved.
- Krea Nodes documentation describes custom node-based workflows that chain image, video, and audio models on an infinite canvas.
- Krea Node App Builder exposes selected workflow parameters as app inputs while hiding intermediate nodes, model switching logic, error handling workflows, and conditional branching.

Useful product pattern:

```text
Canvas workflow
  -> Agent reads current graph and existing outputs
  -> Agent presents plan
  -> User approves
  -> Nodes are placed and wired
  -> Graph is validated
  -> Cost is shown per node
  -> Execution runs
  -> Only downstream nodes rerun after upstream edits
  -> Workflow can be packaged as a reusable app
```

The Krea evidence is public product/documentation evidence. It does not provide a live packet-level backend trace like the Higgsfield test above.

Sources:

- https://www.krea.ai/blog/ai-workflow-agent
- https://docs.krea.ai/user-guide/features/nodes
- https://www.krea.ai/nodes

## Lovart ChatCanvas

Public source evidence:

- Lovart describes ChatCanvas as a real-time infinite workspace where users collaborate with a Design Agent.
- Public launch/news material describes generation and editing of images, videos, audio, brand kits, and 3D renders in one place.
- Public material positions Lovart as a multimodal design agent that coordinates creative tasks on a single canvas.

Useful product pattern:

```text
Prompt/chat first
  -> Agent decomposes design intent
  -> Multimodal tools/models are selected
  -> Outputs appear as editable canvas assets
  -> User iterates through chat and direct canvas manipulation
```

The Lovart evidence here is public/static product evidence only. I did not run an authenticated packet-level Lovart test in this session.

Source:

- https://www.lovart.ai/news/lovart-design-agent-public-launch-chatcanvas

## Helixflow Code Map

Current frontend transport:

- `web/src/api.ts`:
  - `fetchWorkspaceState(workspaceId)` calls `GET /api/workspaces/{workspaceId}/state`.
  - message/run/proposal/confirmation actions are REST posts.
  - `connectWorkspaceEvents(workspaceId)` opens `/ws?workspace_id=...`.
  - inbound messages are parsed as `RunEventEnvelope`.

Current canvas UI:

- `web/src/components/graph-canvas.tsx`:
  - local `view`, `mode`, and `selected` React state.
  - pan gesture updates local view state.
  - selected node is local UI state.
  - rendered graph is either the persisted graph or `pendingProposal.previewGraph`.

Current backend transport:

- `crates/server/src/main.rs`:
  - `/ws` upgrades to WebSocket.
  - server streams broadcast `RunEventEnvelope`.
  - no current client-to-server canvas op receive loop.

Current Agent proposal path:

- `crates/server/src/workbench.rs`:
  - `send_message` classifies chat vs workflow creation/modification.
  - workflow turns call `agent.propose_graph_change`.
  - backend writes `ops.json` and `preview.json`.
  - pending proposal is persisted.
  - `apply_proposal` validates pending/current version before applying.

Current graph ops:

- `crates/graph/src/lib.rs`:
  - graph nodes have `node_type`, `title`, `params`, and `pos`.
  - supported proposal ops include `add_node`, `remove_node`, `set_param`, `add_edge`, `remove_edge`, and `move_node`.
  - graph compiles into an execution plan by topological order.

Current run execution:

- `crates/run/src/lib.rs`:
  - execution emits `run.started`.
  - each step builds a `ProviderRequest`.
  - provider outputs are persisted as artifacts.
  - step updates emit `node.state`.

Current provider/node catalog:

- `crates/server/src/provider.rs`:
  - provider abstraction dispatches `health`, `catalog`, `estimate`, `invoke`, and `cancel`.
  - Atlas capabilities include `chat_completion`, `image_generate`, `image_edit`, `text_to_video`, and `image_to_video`.
- `crates/registry/src/lib.rs`:
  - built-in nodes include `input.text`, `input.image`, `llm.prompt_writer`, `image.atlas.generate`, `video.atlas.text_to_video`, `video.atlas.image_to_video`, and `output.save`.

## Helixflow vs Higgsfield

| Capability | Higgsfield observed | Helixflow current |
| --- | --- | --- |
| Canvas list | `/canvas` list page | Workspace list exists, but not canvas-project model parity. |
| Project route | `/canvas/{canvasId}` | Workspace route/state model, local app. |
| Canvas auth ticket | `GET /api/flow/{id}/auth` returns ticket | Not present. |
| Canvas WS | `wss://canvas-worker.../connect/{id}?ticket=...` | `/ws?workspace_id=...`, server event stream only. |
| Durable canvas op | `node:add`, `batch`, `node:prop` | Proposal ops exist, but no live client op socket. |
| Optimistic pending ops | `pendingOpIds` in sync | Not present. |
| Sequence reconciliation | `lastSeq` in sync | Run event `seq` exists, not canvas op seq. |
| Awareness | cursor/drag/drawing over WS | Not present. |
| Selection | local panel plus detail fetch | local React selected state. |
| Generation backend | FNF asset/job/wallet endpoints | Provider abstraction and local run service. |
| Job result backfill | static `/nodes/from-job` endpoint | Artifacts update run records; no canvas backfill endpoint. |
| Cost gate | wallet/cost UI and paid run buttons | Agent run confirmation/cost gate exists. |
| Agent graph proposal | Krea/Higgsfield-style plan/build implied by UI/product | Strong proposal/apply foundation already present. |

## What Helixflow Can Reuse

Helixflow already has several pieces that should not be discarded:

- Graph operation model with validation.
- Proposal preview/apply workflow.
- Backend source-of-truth posture.
- Cost gate for Agent-triggered paid runs.
- Provider abstraction.
- Artifact persistence tied to run steps.
- Agent session contract with explicit context and output files.

These map well to a canvas-agent product if a live canvas transport is added.

## Missing Architecture For A Higgsfield-Style Canvas Agent

Required missing pieces:

1. Canvas project model.

   Workspaces should distinguish product workspace/conversation from canvas document. A canvas document needs title, owner/workspace, current sequence, current snapshot/version, and a durable op stream.

2. Ticketed canvas socket.

   Add a short-lived auth ticket endpoint before opening a canvas WS. This prevents long-lived app auth tokens from being embedded in every canvas socket URL and gives the backend a place to enforce per-canvas access.

3. Durable op log.

   Store canvas ops as append-only records:

   ```text
   canvas_id
   seq
   op_id
   client_id
   user_id
   base_seq
   op_kind
   payload_json
   created_at
   ```

4. Sync/replay protocol.

   The client should send `sync(lastSeq, pendingOpIds)` on connect. The server should return missing ops after `lastSeq`, acknowledge pending op IDs, and reject or transform conflicting ops deterministically.

5. Ephemeral awareness channel.

   Cursor, selection, drag, drawing, and presence should not be committed to the durable graph. They can share the same WS transport with `type=awareness` or use a separate channel later.

6. Client-side optimistic outbox.

   Each local edit needs a stable `opId`, optimistic application, pending status, retry on reconnect, and reconciliation after server acknowledgement.

7. Canvas op grammar.

   Minimum op kinds:

   ```text
   node:add
   node:remove
   node:move
   node:prop
   edge:add
   edge:remove
   comment:add
   comment:resolve
   batch
   ```

8. Generation job split.

   Canvas worker should not run model jobs directly. It should create or reference jobs through a job backend/run service, then represent job progress/results as canvas node state.

9. `nodes/from-job` equivalent.

   When a job finishes, there needs to be a deterministic endpoint or internal handler that converts a job/artifact result into canvas node creation or node update ops.

10. Media/asset model.

    Asset references should be first-class in canvas nodes. A node should reference artifact/job IDs rather than embedding large provider output blobs in canvas state.

## Recommended Helixflow Target Topology

```text
Browser
  React canvas UI
  local optimistic outbox
  presence/selection state
    |
    | REST /api/canvases
    | REST /api/canvases/{id}/auth
    | WS   /api/canvases/{id}/connect?ticket=...
    v
Canvas service
  auth ticket validation
  append-only op log
  snapshot/replay
  awareness fanout
  node/job bridge
    |
    | job create / job status / artifact callbacks
    v
Run/job service
  provider catalog
  cost estimate
  cost confirmation
  provider execution
  artifact persistence
    |
    v
Provider connectors
  Atlas / ComfyUI / other model backends
```

The existing `Workbench`, `GraphService`, and `RunService` can remain. The new part is a canvas service that converts direct manipulation into durable graph/canvas ops and bridges completed jobs back into the canvas.

## Suggested Implementation Sequence

### Phase 1: Canvas Protocol Foundation

Add data contracts before UI changes:

- `CanvasOpEnvelope`.
- `CanvasSyncRequest`.
- `CanvasAck`.
- `AwarenessEvent`.
- `CanvasSnapshot`.

Add a local-only server implementation:

- `POST /api/canvases`.
- `GET /api/canvases`.
- `GET /api/canvases/{id}`.
- `POST /api/canvases/{id}/auth`.
- `GET /api/canvases/{id}/connect` as WebSocket.

Do not connect generation jobs yet. Prove add/move/select/comment op sync first.

### Phase 2: Durable Op Log And Snapshot

Persist op log and snapshots:

- Append every durable op with monotonic `seq`.
- Rebuild canvas from snapshot plus later ops.
- Support reconnect with `lastSeq`.
- Support duplicate `opId` idempotency.
- Keep awareness outside persistence.

### Phase 3: Frontend Outbox

Add a canvas client store:

- Generate `clientId`.
- Generate stable `opId`.
- Optimistically apply local op.
- Send pending ops over WS.
- Resend pending ops after reconnect.
- Reconcile server seq and acknowledgements.

Keep selection local or awareness-only unless multi-user shared selection is a product requirement.

### Phase 4: Agent Proposal To Canvas Patch

Current Agent proposal ops can become canvas ops:

- `add_node` -> `node:add`.
- `move_node` -> `node:move`.
- `set_param` -> `node:prop`.
- `add_edge` -> `edge:add`.
- `remove_edge` -> `edge:remove`.

The important product decision: direct user edits can be immediate canvas ops, while Agent edits can stay plan-first and require approval before ops are committed.

### Phase 5: Job Backend Split And Backfill

Keep generation execution in `RunService`/provider layer:

- Canvas node requests a job.
- Run/job service creates run/job records.
- Canvas node stores job ID and status.
- Run events update node state.
- Finished artifacts trigger `nodes/from-job` equivalent.

Candidate endpoint:

```http
POST /api/canvases/{canvas_id}/nodes/from-job
```

Candidate payload:

```json
{
  "job_id": "job_x",
  "artifact_id": "artifact_y",
  "target": {
    "mode": "create_node",
    "position": [120, 240]
  }
}
```

The handler should append canvas ops instead of mutating the canvas snapshot directly.

### Phase 6: Collaboration Hardening

Add:

- multi-tab reconnect tests;
- duplicate op replay tests;
- offline pending-op tests;
- sequence gap recovery tests;
- awareness TTL cleanup;
- per-canvas authorization tests;
- no-token-in-log checks.

## Security And Data Rules

Do not log:

- auth tickets;
- Clerk/session tokens;
- provider keys;
- wallet/subscription balances;
- raw prompts from a user's private canvas unless explicitly needed and redacted.

Canvas tickets should be:

- short-lived;
- scoped to one canvas;
- bound to current user/workspace permissions;
- safe to reject and refresh on reconnect.

Canvas op validation must reject:

- unknown node types;
- unknown fields;
- invalid coordinates when bounded canvas rules exist;
- asset references the user cannot access;
- job IDs not owned by the workspace;
- HTML/JS injection in comments, sticky notes, text nodes, and rich text.

## Verification Gaps

The following were not verified and should not be treated as facts:

- Higgsfield move op payload. A drag attempt did not produce a reliable move capture.
- Higgsfield comment op payload.
- Higgsfield paid generation job creation payload.
- Higgsfield live `nodes/from-job` payload and timing.
- Higgsfield server acknowledgement frame schema.
- Higgsfield binary socket format. The captured session used `format=json`.
- Lovart authenticated runtime packet flow.
- Krea authenticated runtime packet flow.

## Side Effect From Live Testing

One empty Sticky Note test node was created in the tested Higgsfield canvas. Undo did not remove it during the session. I did not delete it through the UI because that would be another live cloud data mutation requiring explicit user confirmation.

## Bottom Line

The observed production pattern is:

```text
Canvas UI
  -> ticketed canvas-worker WebSocket
  -> durable incremental ops plus ephemeral awareness
  -> separate generation/job backend
  -> job result backfilled into canvas nodes
```

Helixflow today is:

```text
Workspace UI
  -> REST state/actions
  -> server-to-client run-event WebSocket
  -> Agent proposal/apply
  -> backend run service and provider abstraction
```

The clean path is not to replace Helixflow's proposal/run engine. The clean path is to add a canvas-worker layer with durable op sync and job-result backfill, then map existing proposal ops and run artifacts into that layer.
