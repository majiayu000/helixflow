# GH-Ready Issue Drafts: Canvas Agent Full Feature

Remote publication status:

- These are local issue drafts.
- Do not create remote GitHub issues unless explicitly requested.
- If published, preserve the issue ids below in the body, for example `Spec: CANVAS-001`.

## CANVAS-001: Make React Canvas Consume CanvasDocument As Source Of Truth

### Background

The backend now exposes a durable `CanvasDocument`, but the React canvas still primarily renders from `WorkbenchState.graph`. This keeps the product in a graph-view mode instead of a real canvas-agent editor.

### Scope

- Add a canvas slice to frontend state.
- Fetch `/api/workspaces/{workspace_id}/canvas` during workspace hydration.
- Render `GraphCanvas` from `CanvasDocument` when present.
- Keep existing `WorkbenchState.graph` fallback while migration is incomplete.
- Preserve proposal preview behavior.

### Non-Scope

- Do not implement all editing ops in this issue.
- Do not redesign the entire app shell.

### Acceptance

- Opening a workspace loads backend canvas snapshot.
- `GraphCanvas` can render workflow nodes and edges from `CanvasDocument`.
- Empty graph-only workspaces still render a blank canvas.
- Existing proposal preview UI still works.
- TypeScript build passes.

### Verification

```bash
npm test
npm run build
```

### Spec Links

- `specs/canvas-agent-full/product.md`
- `specs/canvas-agent-full/tech.md`
- `specs/canvas-agent-full/tasks.md#t1-frontend-canvas-source-of-truth`

## CANVAS-002: Convert Canvas Editing Actions Into Durable Canvas Ops

### Background

Canvas add/move/resize/patch/connect/delete must become durable backend `canvas_ops`. Without this, the backend canvas source of truth cannot capture user edits.

### Scope

- Add a client op helper with idempotency keys.
- Implement node add.
- Implement node move.
- Implement node resize.
- Implement node patch from inspector.
- Implement edge add/delete.
- Reconcile optimistic local state with accepted server ops.
- Display server validation errors without corrupting local state.

### Non-Scope

- Comment UX belongs to `CANVAS-003`.
- Run/artifact UI belongs to `CANVAS-004`.

### Acceptance

- Add/move/resize/patch/connect/delete survive page reload.
- Inspector param patch sends `prev` for conflict-sensitive fields.
- Retry reuses idempotency key and does not duplicate ops.
- Stale/conflicting op returns visible error and local state is recoverable.

### Verification

```bash
npm test
npm run build
cargo test -p helixflow-graph
cargo test -p helixflow-server
```

### Spec Links

- `specs/canvas-agent-full/product.md`
- `specs/canvas-agent-full/tech.md#4-operation-mapping`
- `specs/canvas-agent-full/tasks.md#t2-durable-canvas-editing-ops`

## CANVAS-003: Add Comments And Presence Collaboration UX

### Background

The backend has durable comments in `CanvasDocument` and volatile `canvas_presence`, but the UI does not expose comments, cursor/selection, or collaborator presence yet.

### Scope

- Add node/edge/position comment creation.
- Add comment edit/resolve/delete flows.
- Send selection/cursor/viewport presence updates.
- Consume canvas WebSocket presence events.
- Render collaborator cursors and selections.

### Non-Scope

- Full text CRDT is not required.
- Remote identity provider integration is not required.

### Acceptance

- Comments persist and reload from backend canvas snapshot/events.
- Presence updates do not create durable `canvas_ops`.
- Closing/reopening the page restores comments.
- WebSocket presence updates are visible when another client sends them.

### Verification

```bash
npm test
npm run build
cargo test -p helixflow-server
```

### Spec Links

- `specs/canvas-agent-full/product.md`
- `specs/canvas-agent-full/tech.md#5-websocket-and-reconnect`
- `specs/canvas-agent-full/tasks.md#t3-comments-and-presence-ux`

## CANVAS-004: Complete Run And Artifact Backfill UI On Canvas

### Background

Backend `run_request` and `artifact_attach` foundations exist. The product still needs a visible canvas workflow where runs launch from canvas and generated results appear as result state/nodes.

### Scope

- Make canvas Run action emit `run_request`.
- Preserve existing cost gate and confirmation modal.
- Update canvas node runtime from run events and accepted canvas ops.
- Render attached artifacts on the corresponding canvas nodes.
- Reuse artifact preview metadata where possible.
- Ensure generated image/video/text/json artifacts are inspectable from canvas.

### Non-Scope

- Replacing `RunService` or provider abstractions.
- Building new provider integrations.

### Acceptance

- User can run from canvas, approve, and see node runtime update.
- Artifacts attach to originating nodes after run completion.
- Result state persists after reload.
- Duplicate `artifact_attach` retries do not duplicate artifact ids.

### Verification

```bash
cargo test --workspace
npm test
npm run build
```

### Spec Links

- `specs/canvas-agent-full/product.md`
- `specs/canvas-agent-full/tech.md#6-run-and-artifact-backfill`
- `specs/canvas-agent-full/tasks.md#t4-run-and-artifact-backfill-ui`

## CANVAS-005: Add Canvas Ticket, Reconnect, And Sync Hardening

### Background

The current local canvas WebSocket works without a dedicated canvas auth ticket or robust reconnect protocol. A canvas-agent editor needs predictable reconnect, seq catch-up, and optional ticket gating.

### Scope

- Add `POST /api/canvases/{canvas_id}/ticket`.
- Gate canvas WebSocket by ticket when ticket mode is enabled.
- Track last canvas `seq` in frontend canvas state.
- On reconnect, fetch `/events?afterSeq={seq}`.
- Detect seq gaps and recover through event fetch.
- Keep REST fallback for offline WebSocket.

### Non-Scope

- Full remote identity provider integration.
- Long-lived shared secrets in code.

### Acceptance

- Missing/invalid ticket is rejected when ticket mode is enabled.
- Reconnect from stale `seq` catches up without duplicate ops.
- Gap detection fetches missing events before applying new WS events.
- Local development can run with ticket enforcement disabled by env config.

### Verification

```bash
cargo test -p helixflow-server
npm test
npm run build
```

### Spec Links

- `specs/canvas-agent-full/tech.md#7-canvas-auth-ticket`
- `specs/canvas-agent-full/tech.md#5-websocket-and-reconnect`
- `specs/canvas-agent-full/tasks.md#t5-canvas-ticket-reconnect-and-sync-hardening`

## CANVAS-006: Add Migration, Compatibility, And E2E Regression Coverage

### Background

The system must remain compatible with graph-only workspaces while proving the full canvas-agent flow: open, edit, comment, run, result, reload.

### Scope

- Add compatibility tests for graph-only workspace bootstrap.
- Add snapshot replay tests where store seq is ahead of snapshot seq.
- Add reload persistence tests for canvas edits.
- Add end-to-end regression coverage or a deterministic manual verification script.
- Update docs after behavior stabilizes.

### Non-Scope

- New provider integrations.
- UI redesign outside canvas-agent flow.

### Acceptance

- Existing graph-only workspace opens as a valid canvas.
- Snapshot replay catches up missing ops.
- Full canvas-agent story passes:
  - open workspace
  - add node
  - move node
  - connect node
  - add comment
  - run
  - approve
  - artifact appears
  - reload preserves state
- All repo deterministic checks pass.

### Verification

```bash
cargo test --workspace
npm test
npm run build
```

### Spec Links

- `specs/canvas-agent-full/product.md#6-done-when`
- `specs/canvas-agent-full/tech.md#8-migration-and-compatibility`
- `specs/canvas-agent-full/tasks.md#t6-migration-compatibility-and-e2e-regression`
