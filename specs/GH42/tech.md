# Technical Spec: GraphCanvas Viewport Persistence, Wheel Zoom, And Minimap

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/42
Locale: zh-CN

## 输入资料

- `web/src/components/graph-canvas.tsx`
- `web/src/app.tsx`
- `web/src/app.test.tsx`
- `web/src/styles.css`
- `web/src/types.ts`
- `web/src/store.ts`
- `crates/graph/src/lib.rs`
- `crates/server/src/workspace_state.rs`
- `basketikun/infinite-canvas` 的 `InfiniteCanvas`、`canvas-mini-map` 和 viewport 本地保存模式

## 当前实现摘要

- `GraphCanvas` 内部维护本地 `view` state，默认 `{ x: 20, y: 18, z: 0.78 }`。
- 主画布支持 pointer drag 平移。
- 主画布只通过 `+/-` 按钮缩放，缩放以当前 view 为中心做简单加减。
- Viewport 没有按 workspace 持久化，刷新后恢复默认值。
- `GraphCanvas` 不接收 workspace id，因此不能区分不同 workspace 的 viewport。
- Pending proposal 通过 `drawGraph = pendingProposal?.previewGraph ?? graph` 渲染，并通过 base graph 计算 diff class。
- 当前没有 minimap。

## 设计决策

1. Viewport 仍保持前端本地 state，不写入 Rust store。
2. `App` 将 `activeState.workspace.id` 传给 `GraphCanvas`，作为 localStorage key 的一部分。
3. `GraphCanvas` 在 workspace id 变化时读取对应 viewport；不存在或解析失败时使用默认 viewport。
4. `GraphCanvas` 在 viewport 变化后 debounce 写入 localStorage。
5. Wheel zoom 使用鼠标位置锚定 world coordinate，避免缩放时跳走。
6. Minimap 放在 `GraphCanvas` 内部，使用当前 `drawGraph`，因此 review mode 会自然显示 preview graph。
7. Minimap 只负责导航，不改变节点、边或 proposal。
8. 不引入新依赖，保持当前 DOM/SVG 实现。

## Frontend Design

### GraphCanvas props

Extend `GraphCanvasProps`:

```ts
type GraphCanvasProps = {
  workspaceId: string;
  graph: WorkbenchState['graph'];
  pendingProposal: WorkbenchState['pendingProposal'];
  run: NonNullable<WorkbenchState['run']>;
};
```

`App` passes `activeState.workspace.id`.

### Viewport persistence

Use localStorage key:

```ts
helixflow:graph-canvas-view:<workspaceId>
```

Persist only:

```ts
type ViewState = { x: number; y: number; z: number };
```

Rules:

- Parse failures fall back to default viewport and do not throw.
- Missing workspace id falls back to default viewport.
- Stored values are clamped before use.
- Writes are debounced to avoid localStorage churn during drag/wheel.

### Wheel zoom

Add `onWheel` to the root canvas section.

Algorithm:

1. Prevent default page scrolling when the event targets the canvas.
2. Read canvas bounding rect.
3. Convert pointer screen coordinate to local canvas coordinate.
4. Compute world coordinate before zoom:

```ts
worldX = (localX - view.x) / view.z
worldY = (localY - view.y) / view.z
```

5. Clamp next zoom.
6. Recompute `x/y` so the same world coordinate remains under the pointer:

```ts
x = localX - worldX * nextZ
y = localY - worldY * nextZ
```

### Minimap

Add a small internal component in `graph-canvas.tsx` first. Split later only if the file grows too large.

Inputs:

- `nodes: GraphNodeState[]`
- `view: ViewState`
- canvas viewport size measured from root section
- `onViewChange(next: ViewState)`

Behavior:

- Compute graph bounds from node positions and fixed node dimensions.
- Add padding around graph bounds.
- Fit bounds into minimap dimensions.
- Render one small block per node.
- Render a rectangle for current viewport.
- Pointer down/move inside minimap recenters main viewport on the target world coordinate.

Empty graph behavior:

- Hide minimap or render an inert empty-state minimap.
- Do not throw on `Infinity` bounds.

### Styling

Add compact styles to `web/src/styles.css`:

- `.canvas-minimap`
- `.canvas-minimap-node`
- `.canvas-minimap-viewport`

Keep the minimap visually secondary and outside existing inspector/run dock affordances.

## Product Requirement Mapping

| Requirement | Implementation Area | Verification |
| --- | --- | --- |
| PRD-01, PRD-02 | `graph-canvas.tsx` wheel handler and clamp helper | Web tests for anchored zoom and clamp |
| PRD-03, PRD-04 | `App` prop wiring and localStorage helper | Web tests for workspace-scoped restore |
| PRD-05, PRD-06 | Minimap render math | Web render test with multi-node graph |
| PRD-07 | Minimap pointer handler | Web test firing pointer events |
| PRD-08 | Empty graph guard and safe parsing | Web render test |
| PRD-09 | Existing `drawGraph` and diff mapping retained | Web test with pending proposal |

## Risks

- jsdom layout returns zero-sized bounding boxes unless tests mock `getBoundingClientRect`.
- Pointer event coverage in React tests may need focused helper setup.
- LocalStorage persistence can make tests order-dependent; tests must clear keys per case.
- Minimap can overlap existing run dock or outputs on smaller screens if styling is too large.
- The current `GraphCanvas` file may grow; keep the first implementation scoped, then split if it approaches maintainability limits.

## Verification Commands

```sh
cd web && npm test -- app.test.tsx
cd web && npm run build
cargo check --workspace
git diff --check
```

## Rollback Plan

- Remove `workspaceId` prop from `GraphCanvas` and `App`.
- Remove viewport persistence helpers and localStorage writes.
- Remove wheel handler and minimap render path.
- Remove minimap CSS.
- Existing pan, zoom buttons, proposal preview, run status and inspector behavior remain as before.
