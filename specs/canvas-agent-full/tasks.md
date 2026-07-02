# Task Plan: Complete Canvas Agent Experience

## T0. Preconditions

Done when:

- Worktree ownership is clear.
- No remote GitHub issues/PRs conflict with this packet.
- Current backend canvas foundation tests pass before starting implementation.

Verification:

- `gh issue list --repo majiayu000/helixflow --state open --limit 50`
- `gh pr list --repo majiayu000/helixflow --state open --limit 50`
- `cargo test --workspace`
- `npm test`

## T1. Frontend Canvas Source Of Truth

Files:

- `web/src/store.ts`
- `web/src/types.ts`
- `web/src/api.ts`
- `web/src/components/graph-canvas.tsx`
- `web/src/app.tsx`
- `web/src/app.test.tsx`

Tasks:

- Add canvas slice to store.
- Fetch `/api/workspaces/{workspace_id}/canvas` during hydration.
- Render `GraphCanvas` from `CanvasDocument` when available.
- Preserve existing graph view fallback during migration.
- Add tests for canvas snapshot parsing and rendering.

Done when:

- Canvas renders backend `CanvasDocument.nodes` and `CanvasDocument.edges`.
- Existing empty workspace behavior remains blank.
- Existing proposal preview still renders.

Verification:

- `npm test`
- `npm run build`

## T2. Durable Canvas Editing Ops

Files:

- `web/src/store.ts`
- `web/src/api.ts`
- `web/src/components/graph-canvas.tsx`
- `web/src/app.test.tsx`
- `crates/server/src/workbench_canvas.rs` if backend response needs small shape changes

Tasks:

- Add client op helper with idempotency key generation.
- Implement node add.
- Implement node move.
- Implement node resize.
- Implement node patch from inspector.
- Implement edge add/delete.
- Reconcile optimistic state with accepted server ops.
- Add stale/retry behavior tests.

Done when:

- Add/move/resize/patch/connect/delete survive reload through backend snapshot/op log.
- Param edits include `prev` where required.
- Failed op returns visible user error and does not corrupt local state.

Verification:

- `npm test`
- `npm run build`
- `cargo test -p helixflow-graph`
- `cargo test -p helixflow-server`

## T3. Comments And Presence UX

Files:

- `web/src/store.ts`
- `web/src/components/graph-canvas.tsx`
- `web/src/canvas.css`
- `web/src/app.test.tsx`
- `crates/server/src/main.rs`
- `crates/server/src/workbench_canvas.rs`

Tasks:

- Add comment panel or inline comment affordance.
- Implement `comment_add`, `comment_patch`, `comment_delete`.
- Send selection/cursor/viewport presence.
- Consume canvas WS presence events.
- Display collaborator selection/cursor state.

Done when:

- Comments persist and reload.
- Presence does not appear in durable op history.
- Disconnect/reconnect restores comments and resumes presence updates.

Verification:

- `npm test`
- `npm run build`
- `cargo test -p helixflow-server`

## T4. Run And Artifact Backfill UI

Files:

- `web/src/store.ts`
- `web/src/components/graph-canvas.tsx`
- `web/src/components/artifact-stage.tsx`
- `web/src/app.test.tsx`
- `crates/server/src/workbench_canvas.rs`
- `crates/run/src/lib.rs` only if event payloads need more data

Tasks:

- Make Run button emit `run_request` from canvas path.
- Keep cost confirmation modal working.
- Apply runtime status updates to canvas nodes.
- Render `artifact_attach` results as node result state or result nodes.
- Reuse artifact preview metadata from existing outputs.
- Ensure duplicate artifact attach retries are idempotent.

Done when:

- User can run from canvas, approve, and see artifacts attached to nodes.
- Result state persists after reload.

Verification:

- `cargo test --workspace`
- `npm test`
- `npm run build`

## T5. Canvas Ticket, Reconnect, And Sync Hardening

Files:

- `crates/server/src/main.rs`
- `crates/server/src/workbench_canvas.rs`
- `web/src/api.ts`
- `web/src/store.ts`
- `web/src/app.test.tsx`

Tasks:

- Add local `POST /api/canvases/{canvas_id}/ticket`.
- Gate canvas WS by ticket when enabled.
- Track last canvas seq on client.
- On WS reconnect, fetch `/events?afterSeq={seq}`.
- Detect seq gaps and recover by event fetch.
- Add REST fallback when WS is offline.

Done when:

- Reconnect from stale seq catches up without duplicate ops.
- Invalid/missing ticket is rejected when ticket mode is enabled.
- Local dev compatibility remains available by env config.

Verification:

- `cargo test -p helixflow-server`
- `npm test`
- `npm run build`

## T6. Migration, Compatibility, And E2E Regression

Files:

- `crates/store/migrations/*`
- `crates/server/src/workbench_canvas.rs`
- `web/src/app.test.tsx`
- optional `scripts/` or `checks/` if this repo adds e2e checks later

Tasks:

- Add compatibility tests for existing graph-only workspaces.
- Add snapshot seq replay tests.
- Add reload persistence tests.
- Add issue-level acceptance checklist runner or documented manual verification.
- Update docs after behavior stabilizes.

Done when:

- Full story works: open, add, move, connect, comment, run, approve, artifact appears, reload.
- All touched Rust/web tests pass.

Verification:

- `cargo test --workspace`
- `npm test`
- `npm run build`
