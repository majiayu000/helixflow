# Technical Spec: Complete Canvas Agent Experience

## 1. Architecture

The product should use a two-layer model:

- `CanvasDocument`: full editor state, durable source of truth.
- `WorkflowGraph`: execution projection derived from `CanvasDocument`.

Current backend foundation already exists:

- `helixflow_graph::CanvasDocument`
- `helixflow_graph::CanvasOpEnvelope`
- `CanvasDocument::apply_op`
- `CanvasDocument::replay_ops`
- `CanvasDocument::project_workflow_graph`
- `canvases`, `canvas_ops`, `canvas_presence`
- canvas REST and WS endpoints

The remaining work is mainly frontend state migration, UX operation emission, comment/presence consumption, run/artifact visual backfill, and hardening.

## 2. Affected Files

Expected frontend files:

- `web/src/types.ts`
- `web/src/api.ts`
- `web/src/store.ts`
- `web/src/app.tsx`
- `web/src/components/graph-canvas.tsx`
- `web/src/components/artifact-stage.tsx`
- `web/src/app.test.tsx`
- `web/src/canvas.css`
- `web/src/styles.css`

Expected backend files:

- `crates/graph/src/canvas.rs`
- `crates/graph/src/canvas_ops.rs`
- `crates/server/src/workbench_canvas.rs`
- `crates/server/src/main.rs`
- `crates/store/src/canvas_records.rs`
- `crates/store/migrations/0002_canvas.sql`
- `crates/run/src/lib.rs` only if node runtime event shape must be expanded

## 3. Frontend State Design

Add a canvas slice to the existing Zustand store:

```ts
type CanvasSlice = {
  canvasStatus: 'idle' | 'loading' | 'ready' | 'error';
  canvasError: string | null;
  canvas: CanvasDocument | null;
  canvasConnection: ConnectionStatus;
  selectedNodeIds: string[];
  selectedEdgeIds: string[];
  presenceByActor: Record<string, CanvasPresence>;
};
```

Rules:

- `WorkbenchState.graph` remains available for existing panels during migration.
- `GraphCanvas` should render from `CanvasDocument` when present.
- The UI may derive execution graph views from canvas data but must not mutate graph-only state as source of truth.
- Accepted server ops update `canvas.seq`; optimistic changes must reconcile against accepted ops.

## 4. Operation Mapping

UI actions map to ops:

| UI action | Durable op | Presence |
| --- | --- | --- |
| Add text card | `node_add` | no |
| Add workflow node | `node_add` | no |
| Move node | `node_move` | optional cursor |
| Resize node | `node_resize` | optional cursor |
| Inspector param edit | `node_patch` with `prev` | no |
| Edge connect | `edge_add` | no |
| Edge delete | `edge_delete` | no |
| Select node/edge | no | `selection` |
| Comment create | `comment_add` | no |
| Comment resolve/edit | `comment_patch` | no |
| Comment delete | `comment_delete` | no |
| Launch run | `run_request` | no |

Idempotency:

- Every client-submitted op uses a deterministic client UUID.
- Retry must reuse the same idempotency key.
- Client must accept already-recorded server ops as success.

## 5. WebSocket And Reconnect

Flow:

1. Fetch snapshot.
2. Open canvas WS.
3. Apply incoming `op` events if `op.seq == canvas.seq + 1`.
4. If a gap is detected, fetch `/events?afterSeq={canvas.seq}`.
5. Apply missed ops with `CanvasDocument`-equivalent client reducer.
6. Presence messages update volatile UI state only.

Client reducer:

- Prefer generated/shared logic if practical.
- Otherwise mirror backend semantics for supported ops.
- Tests must cover move, patch, add, delete, comment, and artifact attach.

## 6. Run And Artifact Backfill

Current backend can create `artifact_attach` ops after confirmed runs.

Frontend requirements:

- Node runtime status should update from run events and/or `artifact_attach`.
- Artifact ids on a node should render a result strip or child result node.
- Generated image/video artifacts should display preview metadata from existing `outputs`.
- Selecting result nodes should reuse artifact preview logic where possible.

Backend requirements:

- Run events should include enough node id and step id data for UI runtime updates.
- `artifact_attach` should not duplicate artifact ids on retry.
- Existing cost gate must remain unchanged.

## 7. Canvas Auth Ticket

Local MVP:

- Add `POST /api/canvases/{canvas_id}/ticket`.
- Return short-lived opaque ticket stored in process memory or signed local token.
- WS accepts `?ticket=...` and rejects missing/expired ticket when ticket mode is enabled.

Compatibility:

- Local dev may allow no ticket behind an env flag.
- Tests should cover rejected missing/invalid ticket when required.

## 8. Migration And Compatibility

Requirements:

- Existing workspaces with only `WorkflowGraph` bootstrap into `CanvasDocument`.
- Existing workspaces with canvas snapshot continue loading by snapshot + replaying ops after snapshot seq.
- Snapshot seq ahead of store seq must error.
- Store seq ahead of snapshot seq must replay missing ops.

## 9. Test Plan

Rust:

- `cargo test -p helixflow-graph`
- `cargo test -p helixflow-store`
- `cargo test -p helixflow-server`
- `cargo test --workspace`

Web:

- `npm test`
- `npm run build`

End-to-end target:

- Start backend and web.
- Create/open workspace.
- Fetch canvas.
- Add node.
- Move node.
- Add comment.
- Emit run request.
- Confirm run.
- Verify artifact appears in canvas and persists after reload.

## 10. Risks

- Existing worktree contains broad unrelated changes; implementation should be split by issue tranche.
- `main.rs`, `workbench.rs`, `store/src/lib.rs`, and canvas modules are near file-size advisory thresholds.
- Frontend state migration can regress current proposal/run UX if done in one large patch.
- Presence should not be persisted as durable history.
