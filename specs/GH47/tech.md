# Technical Spec: Agent Bounded Canvas Ops Contract

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/47
Locale: zh-CN

## 输入资料

- `crates/agent/src/lib.rs`
- `crates/agent/src/contract.rs`
- `crates/agent/src/prompt_stack.rs`
- `crates/agent/src/service.rs`
- `crates/server/src/workbench_message.rs`
- `web/src/app.tsx`
- `web/src/store.ts`
- `web/src/types.ts`
- `web/src/api.ts`
- `web/src/components/graph-canvas.tsx`
- `web/src/components/chat-pane.tsx`
- GH47 issue body

## 当前实现摘要

- Agent graph 模式已经写入 `ctx/graph.json` 和 node catalog。
- Agent proposal output 已通过 `serde(deny_unknown_fields)` 和 `GraphService.preview_proposal()` 校验。
- `ProposalOp` 已支持 `move_node`，可用于 layout proposal。
- Run request 已通过 backend cost gate 进入 `waiting_confirmation`。
- 前端 selection 存在于 `GraphCanvas` 本地 state，尚未传给 Agent。
- ChatPane 已有 agent log 折叠组，但没有 canvas ops evidence 的稳定标签。
- Agent proposal 路径尚未在已有 pending proposal 时拒绝新 proposal。

## 设计决策

1. 第一版 canvas ops 是内部 Agent 合同，不新增外部 MCP。
2. 新增 `crates/agent/src/canvas_ops.rs`，集中定义 compact canvas state、selection 和 bounded ops schema。
3. `AgentSessionRequest` 增加 optional `canvas_context`，graph 模式写入 `ctx/canvas_state.json` 和 `ctx/canvas_ops.json`。
4. 前端 API 边界使用 camelCase；Agent ctx 文件保持内部 snake_case。
5. `GraphCanvas` 通过 `onSelectionChange` 向 App 提升 selection，App 传给 store 的 `sendMessage`。
6. `post_workspace_message` 对 proposal 模式增加 pending proposal guard。
7. UI 只调整工具日志标签/title，不把工具证据写进主 assistant 气泡。

## Backend / Agent Design

### Canvas context

新增 public structs:

```rust
CanvasOpsContext {
  schema_version: 1,
  graph: CompactCanvasGraph,
  selection: CanvasSelection
}
```

Compact graph 仅包含 node id、node type、title、position 和 edge endpoints，不包含 secrets、provider raw config 或本地路径。

### Bounded ops schema

新增 `CanvasOpsRequest` 和 `CanvasOp` tagged enum:

- `read_state`
- `read_selection`
- `propose_layout { moves }`
- `propose_graph_ops { ops }`
- `run_selected_workflow { node_ids }`

所有 structs 使用 `serde(deny_unknown_fields)`。`propose_graph_ops.ops` 使用 existing `ProposalOp`，继续复用 graph validation。

### Prompt / ctx files

Graph modes write:

- `ctx/canvas_state.json`
- `ctx/canvas_ops.json`

Prompt 中新增 canvas ops section:

- `read_state` -> read `ctx/canvas_state.json`
- `read_selection` -> read `ctx/canvas_state.json.selection.node_ids`
- `propose_layout` -> output `proposal.json` with `move_node` ops
- `propose_graph_ops` -> output `proposal.json` with bounded proposal ops
- `run_selected_workflow` -> output `run_request.json`; backend creates pending confirmation

### Proposal gate

在 `post_workspace_message` 的 Create/Modify/Debug 分支进入 Agent 前检查 `latest_pending_proposal(workspace_id)`。存在 pending 时返回 bad request，不启动新 Agent proposal。

## Frontend Design

### Types/API

新增:

- `CanvasSelectionContext`
- `CanvasMessageContext`

`sendWorkspaceMessage` input 增加 optional `canvasContext`。

### Selection lifting

`GraphCanvas` 增加:

```ts
onSelectionChange?: (nodeIds: string[]) => void
```

App 保存 `selectedCanvasNodeIds`，workspace/version/proposal 切换后由 GraphCanvas 发空 selection。

### Store

`sendMessage(text, canvasContext?)` 把 selection context 和 current graph 一起发到 message API。

### Logs

`ChatPane` 对 `agent_log:canvas_ops` 显示稳定标签和更明确的 group title。主 assistant answer 不显示 raw evidence。

## Product Requirement Mapping

| Requirement | Implementation Area | Verification |
| --- | --- | --- |
| PRD-01 | `canvas_ops.rs`, `create_session_contract` | agent tests |
| PRD-02 | `GraphCanvas`, `App`, `store`, `api` | web tests |
| PRD-03 | `CanvasOp` schema and ctx contract file | Rust tests |
| PRD-04 | prompt stack proposal examples | agent tests |
| PRD-05 | existing `ProposalOp` validation path | existing/new tests |
| PRD-06 | `workbench_message.rs` pending guard | server tests |
| PRD-07 | run request prompt/backend path | agent/server tests |
| PRD-08 | `serde(deny_unknown_fields)` tests | Rust tests |
| PRD-09 | `chat-pane.tsx` log label/title tests | web tests |

## Risks

- Selection is frontend-local; if it is stale, backend must treat it as context only, not authority.
- Adding selection to message API can drift from graph ids; compact context should filter unknown ids against current graph.
- Existing `graph-canvas.tsx` is near the preferred size limit; changes must stay small.
- Pending proposal guard can change behavior for users who previously submitted multiple Agent proposal turns; this is intentional to preserve review semantics.

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

- Remove `canvas_ops.rs`, ctx file writes, prompt section, and tests.
- Remove message API `canvasContext` input and frontend selection lifting.
- Remove proposal pending guard if it blocks intended product flow.
- Existing proposal apply/dismiss, manual proposal, layout save, and run confirmation paths remain unchanged.
