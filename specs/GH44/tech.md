# Technical Spec: Versioned GraphCanvas Node Repositioning

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/44
Locale: zh-CN

## 输入资料

- `web/src/components/graph-canvas.tsx`
- `web/src/app.tsx`
- `web/src/store.ts`
- `web/src/api.ts`
- `web/src/types.ts`
- `web/src/app.test.tsx`
- `web/src/canvas.css`
- `crates/server/src/main.rs`
- `crates/server/src/proposal_routes.rs`
- `crates/server/src/version_routes.rs`
- `crates/server/src/workspace_state.rs`
- `crates/server/src/graph_files.rs`
- `crates/store/src/lib.rs`
- `crates/store/src/proposal_records.rs`
- `crates/graph/src/lib.rs`
- `basketikun/infinite-canvas` 的节点拖动、局部状态和保存分离模式

## 当前实现摘要

- `GraphCanvas` 使用 `drawGraph = pendingProposal?.previewGraph ?? graph` 渲染当前 graph 或 pending proposal preview。
- `GraphCanvas` 已有本地 viewport persistence、wheel zoom、minimap 和单选 inspector。
- `GraphCanvas` 当前只能 pan canvas，不能拖动节点。
- `WorkflowGraph` 的持久坐标在 Rust 类型 `GraphNode.pos: [f32; 2]` 中。
- `workspace_state_value` 从 current version 的 graph JSON 构造前端 `graph.nodes[].position` 和 `workflowGraph`。
- `proposal_routes` 已有“读当前 graph -> 应用 graph op -> validate -> 写 JSON -> 创建 version -> 返回 workspace state”的模式。
- `GraphService::apply_ops` 已支持 `ProposalOp::MoveNode`，可复用为 layout-only graph update。
- Store 已有 `create_version_after`，可基于 expected current version id 做 stale 写入保护。
- Store 已有 `latest_pending_proposal(workspace_id)`，可用于 pending proposal guard。

## 设计决策

1. 节点重排保存为 `VersionSource::Manual` 的新 graph version，label 使用 `Update layout`。
2. API 使用 `POST /api/workspaces/{workspace_id}/versions/layout`，请求体使用 camelCase 边界：

```json
{
  "baseVersionId": "ver_...",
  "positions": [
    { "id": "node_id", "x": 120, "y": 240 }
  ]
}
```

3. 后端只更新已有节点的 `pos`，不接受新增节点、删除节点、边或 params 变化。
4. 后端必须在写 version 前检查：
   - workspace 有 current version。
   - request `baseVersionId` 等于 current version id。
   - workspace 没有 pending proposal。
   - `positions` 非空。
   - 每个 node id 存在且不重复。
   - `x/y` 是 finite number。
5. 后端使用 `GraphService::apply_ops` 生成 moved graph，并调用 `validate_graph`。
6. 后端写入 `workspaces/{workspace_id}/graphs/layout-{baseVersionId}.json` 类似路径，并用 `write_json_file` 得到 hash。
7. 后端调用 `store.create_version_after(..., baseVersionId)` 作为最终并发保护。
8. 前端拖动只维护 `draftPositions`，不修改 `graph` prop。
9. 前端保存成功后由 store 写入返回的 `WorkbenchState`，`GraphCanvas` 在 graph/version 改变时清空 dirty draft。
10. Pending proposal preview 下允许查看/选择 preview 节点，但禁用拖动保存。
11. 为避免 `graph-canvas.tsx` 超过 800 行，新增纯 helper 文件承载 layout draft 计算和测试。

## Backend Design

### Route

新增 `crates/server/src/layout_routes.rs`：

```rust
pub(crate) async fn save_workspace_layout(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
    Json(request): Json<SaveLayoutRequest>,
) -> Result<Json<Value>, ApiError>
```

Route wiring:

```rust
.route("/api/workspaces/{workspace_id}/versions/layout", post(save_workspace_layout))
```

### Request types

```rust
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SaveLayoutRequest {
    base_version_id: String,
    positions: Vec<NodePositionUpdate>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodePositionUpdate {
    id: String,
    x: f32,
    y: f32,
}
```

Validation:

- `base_version_id.trim().is_empty()` -> `400`.
- `positions.is_empty()` -> `400`.
- duplicate id -> `400`.
- unknown id -> `400`.
- non-finite x/y -> `400`.
- pending proposal exists -> `409`.
- current version mismatch -> `409`.

### Data flow

1. Load workspace and current version.
2. Compare `request.base_version_id` with current version id.
3. Check `latest_pending_proposal`.
4. Read current graph JSON.
5. Convert each position update into `ProposalOp::MoveNode`.
6. Apply ops with `GraphService::apply_ops`.
7. Validate moved graph.
8. Write moved graph to `workspaces/{workspace_id}/graphs/layout-{baseVersionId}.json`.
9. Create version with:
   - `source: VersionSource::Manual`
   - `label: "Update layout"`
   - `parent_id: Some(baseVersionId)`
10. Return `workspace_state_value`.

## Frontend Design

### API

Add:

```ts
export type LayoutPositionUpdate = { id: string; x: number; y: number };

export async function saveWorkspaceLayout(
  workspaceId: string,
  input: { baseVersionId: string; positions: LayoutPositionUpdate[] },
): Promise<WorkbenchState>
```

### Store

Add `saveLayout(positions)` action:

- Read current state.
- Reject no-op if no state or empty positions.
- Send `workspace.id`, `workspace.versionId`, `positions`.
- On success, set `{ state: next, status: 'ready', error: null }`.
- On failure, append system error message.

### App

Pass:

```tsx
onSaveLayout={(positions) => runAction(() => saveLayout(positions))}
```

to `GraphCanvas`.

### GraphCanvas interaction

- Replace `selected: string | null` with `selectedIds: Set<string>`.
- `Shift` or platform modifier click toggles node selection for GH44.
- Plain click selects exactly one node.
- Node pointer down:
  - stop canvas pan.
  - if pending proposal exists, only select; do not start node drag.
  - determine active selection set.
  - record pointer origin and each selected node's start position.
- Pointer move:
  - convert screen delta to world delta by dividing by current zoom.
  - update `draftPositions` for selected nodes.
  - render `displayGraph` from `drawGraph` plus `draftPositions`.
- Pointer up/cancel:
  - finish node drag but keep `draftPositions` dirty.
- Save button appears only when dirty and no pending proposal.
- Save sends changed positions sorted by node id for stable tests.
- Save button disables while `layoutSaving` is true.

### Pure helpers

Create `web/src/components/graph-canvas-layout.ts`:

- `applyPositionDrafts(nodes, drafts)`
- `positionUpdatesFromDrafts(baseNodes, drafts)`
- `toggleSelection(currentIds, nodeId, additive)`
- `moveDraftPositions(input)`

These helpers are testable without DOM pointer event dependence.

## Product Requirement Mapping

| Requirement | Implementation Area | Verification |
| --- | --- | --- |
| PRD-01, PRD-03 | `GraphCanvas` node pointer drag and `displayGraph` rendering | Web tests for drag helpers and edge-follow render |
| PRD-02, PRD-05, PRD-07 | `draftPositions`, save button, graph/version effect cleanup | Web tests for dirty/save behavior |
| PRD-04 | selection set and group drag helper | Web tests for multi-node move |
| PRD-06, PRD-08 | `saveWorkspaceLayout`, store action, backend version route | Backend route tests and store action test |
| PRD-09 | backend request validation and stale current version check | Backend route tests |
| PRD-10 | frontend guard plus backend pending proposal guard | Frontend render test and backend route test |

## Risks

- Pointer drag can conflict with existing canvas pan if node events do not stop propagation.
- Viewport zoom means pointer deltas must be converted to graph/world coordinates.
- Pending proposal preview uses preview nodes; saving those positions would overwrite unconfirmed graph changes if guard is incomplete.
- `GraphCanvas` is already large; helper extraction is required to stay under file-size constraints.
- Creating a graph file before `create_version_after` can leave an orphan JSON if the final stale check fails. This is acceptable for now because graph files are content artifacts and current version remains unchanged.
- `GraphService::validate_graph` validates params and ports; layout-only moves should not change execution semantics, but invalid existing graphs must fail closed rather than silently persist.

## Verification Commands

```sh
cd web && npm test -- app.test.tsx
cd web && npm run build
cargo check --workspace
cargo test --workspace
git diff --check
```

## Rollback Plan

- Remove `/api/workspaces/{workspace_id}/versions/layout` route and `layout_routes.rs`.
- Remove frontend `saveWorkspaceLayout` API and store action.
- Remove `onSaveLayout` prop and node drag/draft logic from `GraphCanvas`.
- Remove layout helper and related tests.
- Existing viewport persistence, minimap, proposal preview, run flow, undo/restore and export behavior remain as before.
