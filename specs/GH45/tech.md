# Technical Spec: GraphCanvas Selection, Shortcuts, And Clipboard Ergonomics

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/45
Locale: zh-CN

## 输入资料

- `web/src/components/graph-canvas.tsx`
- `web/src/components/graph-canvas-layout.ts`
- `web/src/components/graph-canvas-navigation.ts`
- `web/src/components/graph-canvas-inspector.tsx`
- `web/src/components/graph-canvas-rendering.ts`
- `web/src/app.test.tsx`
- `web/src/canvas.css`
- GH45 issue body

## 当前实现摘要

- `GraphCanvas` 已维护 `selectedIds: Set<string>`。
- 单节点 pointer down 会使用 `selectionForNodePointer` 处理 Shift/Cmd/Ctrl additive selection。
- 节点拖拽和 layout save 已基于 `displayNodes`。
- Pending proposal preview 通过 `drawGraph = pendingProposal?.previewGraph ?? graph` 渲染。
- `GraphInspector` 当前只显示单节点 inspector。
- `ChatPane` textarea 已有 IME composition guard，但 canvas 尚无全局快捷键 guard。

## 设计决策

1. 不改后端，不改 `GraphCanvas` props。
2. 新增 `graph-canvas-selection.ts` 承载 selection rectangle、shortcut、clipboard pure helpers。
3. 框选触发规则：
   - `edit` 模式下背景拖拽开始框选。
   - Shift/Cmd/Ctrl 背景拖拽在任意模式下开始框选。
   - 其他背景拖拽继续平移 canvas。
4. `review`/pending proposal 下不允许 layout save，但允许 selection 和 copy，并基于 `drawGraph`。
5. Keyboard listener 绑定在可聚焦的 `.p-canvas` section 上，并通过 `isEditableShortcutTarget` 和 IME guard 跳过输入控件。
6. Clipboard 只复制 selection graph fragment 的安全字段：
   - node: `id`, `nodeType`, `title`, `category`, `position`, sanitized `summary`
   - edges: selected node 间的 `id`, `from`, `to`, `kind`
   - omit `provider`
7. Clipboard sanitizer redacts local absolute paths and token-like strings from summaries.

## Frontend Design

### Selection helper

Create `web/src/components/graph-canvas-selection.ts`:

- `selectionRectFromLocalPoints(start, current)`
- `worldRectFromLocalRect(rect, view)`
- `selectedIdsInWorldRect(nodes, rect)`
- `mergeSelection(baseIds, hitIds, additive)`
- `graphShortcutFromEvent(event)`
- `isEditableShortcutTarget(target)`
- `selectionClipboardText(nodes, edges)`
- `sanitizeClipboardText(value)`

### GraphCanvas state

Add:

```ts
type SelectionDragState = {
  pointerId: number;
  start: { x: number; y: number };
  current: { x: number; y: number };
  additive: boolean;
  baseIds: Set<string>;
};
```

Render overlay:

```tsx
{selectionDrag && <div className="selection-rect" style={...} />}
```

### Keyboard handler

Make the root canvas focusable with `tabIndex={0}` and handle `onKeyDown`:

- `escape` -> clear `selectedIds`.
- `select_all` -> `setSelectedIds(new Set(displayNodes.map(node => node.id)))`.
- `fit_view` -> update view using helper.
- `copy` -> copy current selected nodes and selected internal edges.

Skip if:

- event target is textarea/input/select/contenteditable.
- `event.isComposing`, `event.keyCode === 229`, or `event.which === 229`.
- no selected nodes for copy.

### Fit view

Add `fitViewToNodes(nodes, viewportSize)` to `graph-canvas-navigation.ts` or selection helper. If graph is empty, return `DEFAULT_GRAPH_VIEW`; otherwise fit node bounds with padding and clamp zoom.

### Inspector

Update `graph-canvas-inspector.tsx`:

- keep `GraphInspector` for single selected node.
- add `GraphSelectionInspector({ nodes, onClose })` for multiple selected nodes.

### Styling

Add CSS:

- `.selection-rect`
- `.selection-summary-list`
- `.clipboard-status` if a transient copy status is rendered

## Product Requirement Mapping

| Requirement | Implementation Area | Verification |
| --- | --- | --- |
| PRD-01 | selection drag state and rectangle helper | helper tests |
| PRD-02 | existing `selectionForNodePointer` plus tests | existing/new tests |
| PRD-03-PRD-05 | keyboard helper and `GraphCanvas` key effect | shortcut tests |
| PRD-06, PRD-08 | clipboard helper and sanitizer | clipboard tests |
| PRD-07 | shortcut target/IME guard | target guard tests |
| PRD-09 | `GraphSelectionInspector` | static markup test |
| PRD-10 | `displayNodes/drawGraph` selection source | pending proposal test |

## Risks

- Global keyboard listeners can steal shortcuts from chat input if target guard is incomplete.
- Clipboard summaries can accidentally include local paths or token-like values if sanitizer is too narrow.
- Plain background drag selection can break pan; therefore plain selection is limited to edit mode.
- Selection overlay must not start while dragging nodes or toolbar/minimap/inspector controls.

## Verification Commands

```sh
cd web && npm test -- app.test.tsx
cd web && npm run build
cargo check --workspace
cargo test --workspace
git diff --check
```

## Rollback Plan

- Remove `graph-canvas-selection.ts`.
- Remove selection drag and keyboard effect from `GraphCanvas`.
- Revert inspector to single-node only.
- Remove selection CSS and tests.
- Existing GH42/GH44/GH48 behavior remains intact.
