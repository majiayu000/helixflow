# Technical Spec: Workbench Run Queue And Interrupt Controls

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/23
Locale: zh-CN

## 输入资料

- `docs/ISSUES.md`
- `docs/ROADMAP.md`
- `docs/PROMPT_DESIGN.md`
- `crates/run/src/lib.rs`
- `crates/run/src/cost_gate.rs`
- `crates/server/src/run_routes.rs`
- `crates/server/src/main.rs`
- `web/src/api.ts`
- `web/src/store.ts`
- `web/src/app.tsx`

## 当前实现摘要

- `RunService::execute_manual_run` 已能创建并执行 manual run。
- `RunService::interrupt_run` 已能请求中断 active run。
- `RunService::confirm_run` 和 hold route 已覆盖 `waiting_confirmation` 的 confirmation path。
- 前端能显示 run dock、pending confirmation 和 WebSocket node state。
- 当前 workbench 缺少直接 manual Queue endpoint/action，用户可能需要通过 chat run request 间接触发。
- 当前 active run interrupt 没有作为独立 workbench API/UI 行为固定下来。

## 设计决策

1. Manual Queue 是 workbench action，不是 chat turn。
2. Manual Queue 使用 current workspace version 编译 execution plan，并由后端返回 authoritative run payload。
3. Active run interrupt 调用 `RunService::interrupt_run`，只适用于正在执行且存在 interrupt handle 的 run。
4. `waiting_confirmation` 继续使用 approve/hold routes；hold 可以标记 pending run 为 interrupted，但不能替代 active-run interrupt。
5. 前端只做 request state 和 disabled state，不推断业务成功。

## API Contract

### Queue current workflow

```http
POST /api/workspaces/{workspace_id}/runs
```

Request body first version:

```json
{}
```

Success response:

```json
{
  "run": { "...": "RunPayload" },
  "outputs": [],
  "pendingConfirmation": null
}
```

Errors:

- `404` when workspace is missing.
- `409` when workspace has no current version or a run conflict policy rejects another active run.
- `422` when graph cannot compile into an execution plan.
- `500` only for unexpected server errors.

### Interrupt active run

```http
POST /api/runs/{run_id}/interrupt
```

Success response:

```json
{
  "run": { "...": "RunPayload" },
  "outputs": [],
  "pendingConfirmation": null
}
```

Errors:

- `404` when run is missing.
- `409` when run is not active or cannot be interrupted.

## Backend Design

### Server routes

Add routes in `crates/server/src/main.rs`:

- `POST /api/workspaces/:workspace_id/runs`
- `POST /api/runs/:run_id/interrupt`

Implement route handlers in `crates/server/src/run_routes.rs`.

Queue route responsibilities:

1. Load workspace current version.
2. Read current graph JSON through safe graph file helpers.
3. Compile execution plan through graph/run service boundary.
4. Call `RunService::execute_manual_run`.
5. Return `RunConfirmationResponse`-compatible payload with `pendingConfirmation: null`.

Interrupt route responsibilities:

1. Verify run exists.
2. Call `RunService::interrupt_run`.
3. Return latest run outcome or current run payload.
4. Preserve terminal run status if the run already finished before interrupt request.

### Run concurrency

The implementation must make an explicit first-version policy:

- Either reject a second active manual run in the same workspace with `409`.
- Or allow it and ensure event handling is run-id-scoped.

The product spec prefers avoiding duplicate execution from repeated user clicks, so the route should at minimum protect against obvious double-submit while a request is in flight.

### Error handling

Do not silently degrade Queue failures into assistant replies. API errors should be typed and surfaced in the workbench error state.

## Frontend Design

### API client

Add functions in `web/src/api.ts`:

- `queueWorkspaceRun(workspaceId: string)`
- `interruptRun(runId: string)`

Both should parse the same response schema used by confirmation APIs.

### Store

Add actions in `web/src/store.ts`:

- `queueRun()`
- `interruptRun(runId?: string)`

Store responsibilities:

- Set `busy`/request state for button disabling.
- Apply returned `run`, `outputs`, and `pendingConfirmation`.
- Preserve existing WebSocket handling for progress updates.
- Store API errors in existing user-visible error path.

### UI

Wire controls in the restored workbench shell:

- Queue button in top bar or run dock.
- Interrupt button only enabled for active `running`/`queued` state where backend can accept interrupt.
- Confirmation modal remains responsible for `waiting_confirmation`.

## Data Flow

1. User clicks Queue.
2. Web calls `POST /api/workspaces/{id}/runs`.
3. Server compiles current graph and creates run.
4. RunService emits events through event bus.
5. WebSocket updates graph/run step state.
6. User clicks Interrupt while active.
7. Web calls `POST /api/runs/{id}/interrupt`.
8. RunService marks interrupt handle; execution loop emits `run.interrupted`.
9. Store applies terminal run status from response/event.

## Product Requirement Mapping

| Requirement | Implementation Area | Verification |
| --- | --- | --- |
| PRD-01, PRD-02 | `web/src/api.ts`, `web/src/store.ts`, UI button, `run_routes.rs` | Web test asserts Queue does not call messages API |
| PRD-03, PRD-04 | `run_routes.rs`, `RunService::interrupt_run`, modal wiring | Server route tests for active vs waiting-confirmation |
| PRD-05 | API error mapping and store error state | Web test for rejected Queue/Interrupt |
| PRD-06, PRD-07 | Store busy state and backend conflict policy | Unit tests for repeated calls/concurrent status |
| PRD-08 | Response schemas and WebSocket event reducer | Existing plus new app tests |

## Risks

- Manual run may conflict with agent-requested run confirmation semantics.
- Interrupt can race with natural run completion.
- If the server response is returned before execution finishes, tests must assert event-driven state rather than synchronous terminal state.
- Reusing confirmation response types may hide manual-run-specific fields if not documented clearly.

## Verification Commands

```sh
cargo fmt --check
cargo test -p helixflow-run
cargo test -p helixflow-server run_routes
cargo test -p helixflow-server workspace_state
cd web && npm test -- app.test.tsx
cd web && npm run build
git diff --check
```

## Rollback Plan

- Remove the two new routes and API client/store actions.
- Leave existing chat run request and confirmation routes unchanged.
- Revert UI buttons to disabled placeholders if needed.
- Existing run service tests should remain valid because `RunService` behavior predates this route wiring.
