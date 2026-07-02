# Product Spec: Complete Canvas Agent Experience

## 1. Goal

Helixflow should behave like a real canvas-agent workspace: the visible canvas is the product source of truth, user and agent edits are durable incremental operations, generation jobs are launched from the canvas, and generated results return to the canvas as inspectable nodes.

## 2. Current State

Already implemented:

- Backend `CanvasDocument` format.
- Backend `CanvasOpEnvelope` format.
- SQLite `canvases`, `canvas_ops`, and `canvas_presence`.
- REST APIs for canvas snapshot, ops, events, and presence.
- Canvas WebSocket endpoint.
- Backend `run_request` op integration with existing `RunService`.
- Backend `artifact_attach` backfill after run confirmation.
- Typed frontend API/schema boundary.

Not complete:

- React canvas still renders mainly from `WorkbenchState.graph`.
- Canvas UI actions do not yet emit durable `canvas_ops`.
- Selection and presence are local-only or missing in the canvas UI.
- Comments are not yet a first-class UX.
- Generated artifacts do not yet appear as full canvas result nodes in the editor.
- There is no dedicated canvas session ticket/reconnect UX.
- End-to-end coverage does not yet prove the full add/move/comment/run/result workflow.

## 3. Personas

- Workflow author: creates and edits visual generation workflows.
- Agent user: asks the agent to create or modify nodes, then reviews/apply changes.
- Reviewer/collaborator: watches cursor/selection/comment activity and leaves comments.
- Operator/developer: debugs durable op logs, run events, artifacts, and reconnect behavior.

## 4. User Stories

### 4.1 Open Canvas

As a workflow author, I can open a workspace and see the backend `CanvasDocument`, not a derived-only graph view.

Acceptance:

- Workspace load fetches `/api/workspaces/{workspace_id}/canvas`.
- The React canvas state includes canvas `seq`, nodes, edges, comments, runtime, and metadata.
- Existing graph-only workspaces bootstrap into a canvas without losing existing nodes/edges.
- Empty workspaces show a blank canvas state, not fake demo content.

### 4.2 Edit Canvas

As a workflow author, I can add, move, resize, patch, and delete canvas nodes and edges, and each durable change becomes a backend op.

Acceptance:

- Add text/workflow/artifact/comment/group node emits `node_add`.
- Move emits `node_move`.
- Resize emits `node_resize`.
- Inspector edits emit `node_patch` with `prev` for critical `params`.
- Edge create/delete emits `edge_add` / `edge_delete`.
- Delete node removes or rejects dependent edges according to backend rules.
- UI updates optimistically only when it can reconcile with accepted server ops.

### 4.3 Comments And Presence

As a collaborator, I can select nodes, see active cursors/selections, and leave comments on nodes/edges/positions.

Acceptance:

- Selection/cursor/viewport uses `/presence` or canvas WS volatile messages, not durable `canvas_ops`.
- Comments use durable `comment_add`, `comment_patch`, and `comment_delete`.
- Resolved comments remain queryable until deleted.
- Reconnect restores durable comments from snapshot/events.

### 4.4 Run From Canvas

As a workflow author, I can launch generation from the canvas, see run state on nodes, and inspect results as canvas nodes/artifacts.

Acceptance:

- Run button emits `run_request`.
- Backend projects current `CanvasDocument` to `WorkflowGraph`.
- Existing cost gate/confirmation still applies.
- Run step events update corresponding node runtime states.
- Generated artifacts become visible canvas artifacts/result nodes.
- Users can select a result node and see artifact metadata/preview.

### 4.5 Reconnect And Sync

As a user with an interrupted connection, I can reconnect without losing edits or seeing stale state.

Acceptance:

- Client tracks last accepted canvas `seq`.
- On reconnect, client calls `/events?afterSeq={seq}` before resuming.
- Idempotency keys prevent duplicate ops after retry.
- Stale `base_seq` returns actionable client behavior, not silent overwrite.
- WebSocket failure degrades to REST polling/fetch without corrupting state.

### 4.6 Agent Edits

As an agent user, I can let the agent modify the canvas while preserving reviewability.

Acceptance:

- Agent proposals can be converted to canvas ops or a canvas preview.
- Applying a proposal records durable canvas ops.
- Dismissing a proposal leaves canvas state unchanged.
- Agent-authored ops include actor kind `agent`.

## 5. Non-Goals

- Full multiplayer text CRDT in this tranche.
- Remote identity provider integration beyond a local canvas ticket.
- Replacing existing `RunService`, `GraphService`, provider, cost ledger, or artifact storage.
- Rewriting the whole frontend shell.

## 6. Done When

- A user can open a workspace, add a node, move it, connect it, comment on it, run it, confirm the run, see generated artifacts on canvas, reload the page, and see the same state restored from backend data.
- The same flow is covered by Rust backend tests and web tests.
- Existing graph/proposal/run tests still pass.
