# Technical Spec: Output Selection And Artifact Preview

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/25
Locale: zh-CN

## 输入资料

- `crates/store/src/run_records.rs`
- `crates/server/src/workbench_payload.rs`
- `crates/server/src/workspace_state.rs`
- `crates/server/src/run_routes.rs`
- `crates/server/src/main.rs`
- `web/src/api.ts`
- `web/src/store.ts`
- `web/src/app.tsx`
- `web/src/components/run-panels.tsx`
- `web/src/components/artifact-stage.tsx`
- `web/src/types.ts`

## 当前实现摘要

- `artifacts` 表已有 `selected` 字段。
- `Store::update_artifact_selected` 只能更新单个 artifact，不能保证同 run 单选。
- RunService 的 `output.save` 会创建 selected artifact；provider raw artifacts 默认 `selected=false`。
- Workspace state 当前只返回 latest run artifacts。
- `ArtifactStage` 已存在，但没有被 `App` 渲染。
- `OutputsStrip` 当前是静态 div，不触发后端 selection。

## 设计决策

1. Selection API 使用 `POST /api/outputs/{id}/select`。
2. Server 只允许选择当前 workspace latest run 的 artifact，避免选择后返回 state 看不到 selected artifact。
3. Store 提供原子-ish 单选 helper：同 run artifacts 全部取消，目标 artifact 设为 selected。
4. `storageUri` 对前端暴露为 `/api/outputs/{id}/download`，不返回 provider/local storage path。
5. Workspace state preview 只返回小型安全 summary，真实 artifact 内容不进入 state JSON。
6. HTML preview 使用 sandbox iframe 且不允许 scripts。

## API Contract

### Select output

```http
POST /api/outputs/{output_id}/select
```

Success response: full `WorkbenchState` JSON.

Errors:

- `404` when artifact is missing.
- `409` when artifact is not attached to the latest workspace run.

### Preview output

```http
GET /api/outputs/{output_id}/preview
```

Success response:

```json
{
  "kind": "text",
  "content": "..."
}
```

First version returns lightweight metadata summary only.

### Download output

```http
GET /api/outputs/{output_id}/download
```

First version returns a safe JSON handoff containing artifact id/kind/title/storageUri. It does not expose provider-local path values.

## Backend Design

Add `crates/server/src/artifact_routes.rs`:

- `select_output(Path(output_id), State(state))`
- `preview_output(Path(output_id), State(state))`
- `download_output(Path(output_id), State(state))`

Add store helper in `run_records.rs`:

- `select_run_artifact(artifact_id)`: fetch target artifact, update all artifacts in its run so only target has `selected=1`, return refreshed target artifact.

Enhance output payload generation:

- Add optional `mime` and `preview` fields to `OutputPayload`.
- `storageUri` becomes `/api/outputs/{id}/download`.
- `preview` is small generated summary from artifact metadata, dimensions, duration, and kind.
- If an artifact storage URI appears local absolute, never include it in response.

## Frontend Design

### API Client

Add `selectOutput(outputId: string): Promise<WorkbenchState>`.

### Store

Add `selectOutput(outputId: string): Promise<void>` action that replaces state from server response and surfaces errors through the existing system error path.

### UI

- Render `ArtifactStage` inside the canvas area before `GraphCanvas` when selected output has preview.
- Make OutputsStrip items buttons.
- Clicking an output calls store `selectOutput`.
- Current selected output remains visually marked.

## Product Requirement Mapping

| Requirement | Implementation Area | Verification |
| --- | --- | --- |
| PRD-01, PRD-02 | `artifact_routes.rs`, `run_records.rs` | Server route/store tests |
| PRD-03 | `workspace_state_value` after selection | Server route test |
| PRD-04 | `app.tsx`, `artifact-stage.tsx`, `run-panels.tsx` | Web app tests |
| PRD-05, PRD-06 | `workbench_payload.rs` | Server payload tests |
| PRD-07 | `artifact-stage.tsx` sandbox | Web static render test |
| PRD-08 | `artifact_routes.rs` latest-run guard | Server route test |

## Risks

- First version preview is metadata-only; real thumbnails/signed downloads need a future storage layer.
- Selecting only latest-run artifacts is conservative; older-run artifact restore can be added later if History grows artifact browsing.
- Existing `storage_uri` values may be workspace pseudo-URIs; the API still hides them behind download route for consistency.

## Verification Commands

```sh
cargo fmt --check
cargo check --workspace
cargo test -p helixflow-store run_records
cargo test -p helixflow-server artifact_routes
cargo test --workspace
cd web && npm test -- app.test.tsx
cd web && npm run build
git diff --check
```

## Rollback Plan

- Remove artifact routes and frontend selection action.
- Revert OutputsStrip to static display and remove ArtifactStage from App.
- Keep existing artifact table and selected field unchanged.
