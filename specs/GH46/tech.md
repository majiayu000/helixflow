# Technical Spec: Manual Graph Proposal Editing Controls

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/46
Locale: zh-CN

## 输入资料

- `crates/graph/src/lib.rs`
- `crates/server/src/proposal_routes.rs`
- `crates/server/src/main.rs`
- `crates/server/src/workbench_payload.rs`
- `crates/server/src/workspace_state.rs`
- `crates/registry/src/lib.rs`
- `web/src/api.ts`
- `web/src/store.ts`
- `web/src/types.ts`
- `web/src/app.tsx`
- `web/src/components/chat-pane.tsx`
- `web/src/app.test.tsx`
- GH46 issue body

## 当前实现摘要

- `GraphService.preview_proposal()` 已能 apply `ProposalOp` 并调用 `validate_graph()`。
- `ProposalOp` 已支持 `add_node`、`remove_node`、`set_param`、`add_edge`、`remove_edge`、`move_node`。
- Proposal apply/dismiss routes 已存在并创建 version 或保留 current graph。
- `workspace_state_value()` 已返回 `workflowGraph` 和 pending proposal preview。
- 前端 store 已有 `applyProposal`、`dismissProposal`，但没有创建手动 proposal 的 API/action。
- 前端没有 node registry catalog endpoint 或 UI controls。

## 设计决策

1. 新增后端 manual proposal creation route，不改变 apply/dismiss route。
2. 手动 route 只接受单个受限 op，后端转换为 existing `ProposalOp`，再调用 `GraphService.preview_proposal()`。
3. 手动 route 在已有 pending proposal 时拒绝，避免 proposal 并发冲突。
4. 新增 node registry catalog read route，前端只用它构建 add-node UI；后端仍是最终校验源。
5. 前端新增 `ManualProposalPanel` 组件，不把手动编辑 UI 放进 `GraphCanvas`。
6. 前端使用 `workflowGraph` 读取 current params/edges，生成 set-param `prev` 和 edge request。
7. UI 提交失败必须显示错误信息；不进行本地 graph mutation。

## Backend Design

### Routes

- `GET /api/registry/catalog`
  - returns `NodeRegistry::builtin().export_catalog()`.
- `POST /api/workspaces/{workspace_id}/proposals/manual`
  - request:

```json
{
  "baseVersionId": "ver_1",
  "title": "Manual graph change",
  "summary": "Add node input.text",
  "op": { "op": "add_node" }
}
```

### Manual op request shape

- `add_node`: `{ id, nodeType, title?, params, pos }`
- `remove_node`: `{ id }`
- `set_param`: `{ id, key, value }`
- `add_edge`: `{ from, to, edgeType }`
- `remove_edge`: `{ from, to, edgeType }`

Backend maps these to `ProposalOp`.

### Validation and storage

1. Read workspace current version and current graph.
2. Ensure `baseVersionId == current version`.
3. Ensure no latest pending proposal exists.
4. Convert manual request to `ProposalDraft`.
5. Call `GraphService::preview_proposal(&current_graph, current_version_id, draft)`.
6. Persist ops and preview graph under `workspaces/{workspace_id}/proposals/manual-{id}/`.
7. Create proposal record and proposal_pending message.
8. Return `workspace_state_value()` with the freshly prepared `pendingProposal` carrying diff summary.

## Frontend Design

### Types/API

- Add `NodeCatalogSchema`, `NodeDefinitionSchema`, `ManualProposalRequest` types.
- Add `fetchNodeCatalog()`.
- Add `createManualWorkspaceProposal(workspaceId, input)`.
- Add store action `createManualProposal(input)` that replaces state with returned `WorkbenchState` or appends system error.

### UI

Create `web/src/components/manual-proposal-panel.tsx`.

Controls:

- operation segmented/select control.
- add node: node type select from catalog, node id, title, params JSON, x/y.
- remove node: node select.
- set param: node select, key select/input, value JSON.
- add edge: from node/port, to node/port, edge type.
- remove edge: existing edge select.

State:

- Disabled when busy, no `workflowGraph`, or pending proposal exists.
- Shows local JSON parse errors and API errors.
- Does not mutate `graph` locally.

## Product Requirement Mapping

| Requirement | Implementation Area | Verification |
| --- | --- | --- |
| PRD-01 | `ManualProposalPanel`, store action | frontend render/store tests |
| PRD-02 | `GET /api/registry/catalog`, node type select | API/schema test |
| PRD-03 | manual op request union and UI modes | frontend helper/API tests |
| PRD-04 | backend create route only creates proposal record | Rust route tests |
| PRD-05 | route response and existing ProposalCard | app SSR/store tests |
| PRD-06 | route calls `GraphService.preview_proposal()` | Rust invalid params/edge tests |
| PRD-07 | explicit error mapping | Rust and frontend tests |
| PRD-08 | existing apply/dismiss route coverage | existing/new Rust tests |
| PRD-09 | panel/store error rendering | frontend test |

## Risks

- Duplicating node catalog in frontend would drift; use backend catalog endpoint.
- Missing `workflowGraph` would make set-param `prev` unreliable; disable with explicit message.
- Multiple pending proposals would create confusing preview/apply semantics; reject creation while one is pending.
- Raw JSON params can expose parse friction; keep first version small and explicit.

## Verification Commands

```sh
cd web && npm test -- app.test.tsx
cd web && npm run build
cargo fmt --check
cargo check --workspace
cargo test --workspace
git diff --check
```

## Rollback Plan

- Remove manual proposal route and catalog route.
- Remove frontend panel, API/store action, and types.
- Existing agent proposal apply/dismiss remains unchanged.
