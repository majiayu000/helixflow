# Tech Spec

## Linked Issue

GH72。

## Product Spec

`product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| App shell | `web/src/app.tsx` | 组合 `TopBar`、`ChatPane`、`GraphCanvas`、`ManualProposalPanel`、history、confirm、run、outputs | 新设计仍应复用这个 composition，减少重写风险 |
| Top bar | `web/src/components/top-bar.tsx` | workspace tab、provider、connection、undo/history/export/run | 需要承载 version、dirty edit、cost、Queue lock reason |
| Chat | `web/src/components/chat-pane.tsx`, `web/src/chat-log.css` | chat messages、proposal card、run error card、IME-safe submit | 需要加 session/change summary、prompt diff、node-context dialog 入口 |
| Canvas | `web/src/components/graph-canvas.tsx`, `web/src/canvas.css` | pan/zoom/minimap、view/edit/review、selection、node drag layout draft、copy selection | 是新 UI 的主工作面，需要扩展 edit session 和 canvas native tools |
| Inspector | `web/src/components/graph-canvas-inspector.tsx` | 选中节点/多选只读摘要 | 需要迁移为 schema-driven edit surface 或 node dialog trigger |
| Manual proposal | `web/src/components/manual-proposal-panel.tsx`, `manual-proposal-helpers.ts` | 表单式 add_node/set_param/add_edge proposal | 可作为 backend op helper 的过渡层，主 UI 不应继续暴露为大面板 |
| API/types | `web/src/api.ts`, `web/src/types.ts` | `WorkbenchState`, `ManualProposalInput`, run/confirm/export/layout/output APIs | 需要新增 edit session schema 或 batch op commit API |
| Store | `web/src/store.ts` | bootstrap/send/apply/dismiss/queue/undo/layout/output state mutation | 需要集中管理 dirty ops、commit/discard、Queue disabled reason |
| Run/output panels | `web/src/components/run-panels.tsx`, `artifact-stage.tsx` | confirm modal、run dock、history、output preview/select | 需要映射 `2b` Cost Gate、`2c` 输出评审、`2d` 失败诊断 |
| Existing canvas spec | `specs/canvas-agent-full/` | durable canvas source-of-truth 设计 | UI refresh 应与该方向兼容，不另起一套 canvas truth |

## 设计方案

### 1. UI Shell

新增或重组 workbench shell 时优先保持现有 component boundary：

- `App` 继续负责 workspace bootstrap、event stream、top-level actions。
- `TopBar` 变成 state-rich header，新增 props:
  - `versionLabel`
  - `dirtyEditCount`
  - `queueLockReason`
  - `costSummary`
  - `providerSummary`
- `ChatPane` 继续是左侧 session dock，但增加 optional `editSessionSummary` 和 proposal diff render。
- `GraphCanvas` 继续是中心舞台，但内部 toolbar 迁移为 icon-first director toolbar。
- `ManualProposalPanel` 在第一轮可折叠为 advanced/debug，不作为主编辑入口。

CSS 应先引入 tokens，避免在每个组件中复制设计稿 inline style：

- `--hf-bg`
- `--hf-panel`
- `--hf-panel-2`
- `--hf-border`
- `--hf-text`
- `--hf-text-muted`
- `--hf-accent`
- `--hf-accent-soft`
- `--hf-success`
- `--hf-danger`
- `--hf-radius-sm`
- `--hf-radius-md`

### 2. Queue Lock Reason

当前 `queueDisabled` 是 boolean 条件。需要改成可解释的 derived state：

```ts
type QueueLockReason =
  | { kind: 'none' }
  | { kind: 'busy' }
  | { kind: 'active_run' }
  | { kind: 'empty_graph' }
  | { kind: 'provider_unavailable'; message: string }
  | { kind: 'pending_confirmation' }
  | { kind: 'pending_proposal' }
  | { kind: 'dirty_edits'; count: number };
```

UI 从 reason 派生 disabled/title/copy，避免多个组件各自猜原因。

### 3. Manual Edit Session

新增 frontend state：

```ts
type EditSession = {
  baseVersionId: string;
  source: 'user';
  ops: ManualEditOp[];
  startedAt: string;
};
```

`ManualEditOp` 应使用 snake_case 字段，以便和 API 边界一致：

```ts
type ManualEditOp =
  | { op: 'move_node'; id: string; x: number; y: number }
  | { op: 'set_param'; id: string; key: string; value: unknown }
  | { op: 'add_node'; id: string; node_type: string; title?: string; params: unknown; pos: [number, number] }
  | { op: 'remove_node'; id: string }
  | { op: 'add_edge'; from: [string, string]; to: [string, string]; edge_type: string }
  | { op: 'remove_edge'; from: [string, string]; to: [string, string]; edge_type: string };
```

第一轮可复用 `ManualProposalInput` 的 op union，但不要继续把每次用户编辑立即变成 Agent proposal。正确路径是：

1. User action creates local op.
2. Canvas renders preview from committed graph + local ops.
3. Queue locks with `dirty_edits`.
4. Commit sends batch ops with `base_version_id`.
5. Server validates against current version and node catalog/schema.
6. Server creates new version with `source=user`.
7. API returns full `WorkbenchState`.

如果后端 batch commit 尚未准备好，第一批 PR 可以先隐藏 commit behind feature flag，但不得把 dirty state 当成 committed state。

### 4. Canvas Native Controls

在 `GraphCanvas` 内部拆分子组件，避免主文件继续膨胀：

- `graph-canvas-toolbar.tsx`
- `graph-canvas-selection-toolbar.tsx`
- `graph-canvas-node-toolbar.tsx`
- `graph-canvas-node-dialog.tsx`
- `graph-canvas-edit-session.ts`

工具行为：

- select/pan 复用现有 pointer capture 和 view state。
- box select 复用 `graph-canvas-selection.ts`。
- move node 从 `draftPositions` 迁移为 `move_node` op。
- copy/paste/delete 产生 local ops，commit 前不改变 server version。
- connect mode 从 port drag 产生 `add_edge` op；空白释放时打开 add-node chooser。
- add-node chooser 使用 `/api/registry/catalog`；失败时禁用控件并显示错误。

### 5. Node Dialog

Node dialog 不应绕过 proposal/review gate。推荐两个动作：

- Text-only rewrite: 发送 `canvasContext.selection.nodeIds` 和 declared upstream context，Agent 返回 proposal。
- Direct field edit: 产生 `set_param` dirty op，等待 user commit。

Node dialog payload 必须严格 schema 校验。现有 `CanvasMessageContextSchema.strict()` 已有基础，新增字段要显式声明，例如：

```ts
selection: {
  nodeIds: string[];
  includeUpstreamDepth?: number;
}
```

不要允许任意 `Any` payload 或未声明字段。

### 6. Cost Gate / Output / Failure

- `ConfirmModal` 改造成 `CostGatePanel` 或保留 component 名但改视觉。
- `PendingConfirmation` 已包含 `cost`、`runCount`、`pendingChanges`、`interruptible`，UI 应完整展示。
- `OutputsStrip` 和 `ArtifactStage` 应支持 `screening room` 模式：large preview、seed list、Agent recommendation、select output。
- `RunErrorCard` 增加 `Create minimal fix proposal` action，该 action 仍走 `sendWorkspaceMessage` 或 dedicated debug route，不直接修改 graph。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 | `App`, bootstrap, empty state | `cd web && npm test -- app.test.tsx` empty workspace tests |
| P2 | `TopBar`, queue reason helper | render tests for top bar states |
| P3 | `ChatPane`, `GraphCanvas`, queue reason | pending proposal render + queue locked test |
| P4-P7 | `store.ts`, edit session helpers, commit API | unit tests for dirty ops, commit/discard, version update |
| P8 | canvas toolbar components | component tests for disabled reason and tool modes |
| P9 | node dialog, `CanvasMessageContextSchema` | schema unknown-field rejection + node dialog send tests |
| P10 | confirm/cost gate | pending confirmation render and no-run-before-confirm tests |
| P11 | outputs select flow | `selectOutput` state update + render persistence test |
| P12 | failed run card/debug route | failed run render + fix proposal action test |
| P13 | catalog fetch error path | catalog failure disables add/edit controls |

## 数据流

Manual edit commit:

1. `GraphCanvas` captures user action.
2. `useWorkbenchStore` appends `ManualEditOp` to `editingSession`.
3. UI renders preview from committed graph plus ops.
4. `TopBar` derives `QueueLockReason`.
5. User clicks commit.
6. API sends `{ base_version_id, source: 'user', ops }`.
7. Backend validates version, op schema, node ids, ports, catalog schema.
8. Backend writes new version and returns `WorkbenchState`.
9. Store clears `editingSession` and refreshes workspace/history.

Agent proposal path:

1. User sends chat or node dialog request.
2. Frontend sends declared `canvasContext`.
3. Agent returns proposal, not direct graph mutation.
4. UI previews proposal and locks Queue until apply/dismiss.

Run path:

1. Queue action checks no dirty edits, no pending proposal, provider ready.
2. Server returns `pendingConfirmation` if cost gate applies.
3. User confirms.
4. Run events update graph/run/output state.
5. Outputs can be selected and persisted.

## 备选方案

- Only CSS refresh: fastest, but fails the actual design because manual editing/version semantics remain missing.
- Replace canvas with third-party graph editor: may speed up interactions but risks breaking existing proposal/run graph mapping and current tests.
- Keep `ManualProposalPanel` as primary UI: low code churn, but does not match the canvas-native design and keeps users in form mode.

## 风险

- Security: node dialog and manual ops must not accept arbitrary code, HTML, or undeclared payload fields.
- Compatibility: existing workspaces and proposal preview must keep loading while edit session rolls out.
- Performance: rendering preview from committed graph + many local ops may need memoized reducers.
- Maintenance: `GraphCanvas` is already large; new controls should be split into focused files.
- Data correctness: dirty edits must never run against stale `baseVersionId` silently.

## 测试计划

- [ ] Unit tests: queue reason helper, edit session reducer, op preview reducer, node dialog schema.
- [ ] Component tests: TopBar states, dirty edits summary, pending proposal lock, catalog error disabled controls.
- [ ] Integration tests: commit manual edits -> new version -> history update; discard -> no version.
- [ ] Manual verification: open workspace, edit, commit, run, confirm cost, inspect output, fail a run and request fix.

## 回滚方案

- Keep old component boundaries and old route APIs until new edit session is stable.
- Gate canvas-native editing behind a feature flag or config flag if needed.
- Roll back visual tokens separately from edit-session behavior.
- If batch commit fails in production, disable commit controls and keep read/review mode available; do not fallback to running stale graph.
