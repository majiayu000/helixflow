# Technical Spec: Split GraphCanvas Components And Improve Large-Graph Rendering

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/48
Locale: zh-CN

## 输入资料

- `web/src/components/graph-canvas.tsx`
- `web/src/components/graph-canvas-navigation.ts`
- `web/src/components/graph-canvas-layout.ts`
- `web/src/app.test.tsx`
- `web/src/canvas.css`
- `web/src/types.ts`
- GH42/GH44 SpecRail docs
- `basketikun/infinite-canvas` 的组件拆分和 minimap/node boundary 参考

## 当前实现摘要

- `graph-canvas.tsx` 约 664 行，仍包含主 canvas、toolbar、edge SVG、node card、inspector、minimap 和部分 pure helper。
- GH44 已新增 `graph-canvas-layout.ts` 承载 layout draft helper。
- GH44 已新增 `graph-canvas-navigation.ts` 承载 viewport/minimap math helper。
- `GraphCanvas` 内部已经用 `nodeById` map 渲染 edges。
- `WorkflowNode` 渲染时仍使用 `run.steps.find(...)`，在大 graph + 大 run steps 下是 O(nodes * steps)。
- Pending proposal diff 使用 `baseNodeById` 和 `baseEdgeIds`，应该继续保留。

## 设计决策

1. 不改变 `GraphCanvas` 对外 props 和用户行为。
2. 保留当前 DOM/SVG 渲染方式，不引入 canvas library。
3. 只做拆分和 lookup 优化，不做真正 viewport culling；culling 留给后续更明确的性能 issue。
4. 新模块命名保持 `graph-canvas-*` 前缀，避免散落。
5. `GraphCanvas` 继续 re-export navigation helpers，保持现有测试 import 不回退。

## Frontend Design

### New files

- `web/src/components/graph-canvas-node.tsx`
  - `WorkflowNode`
  - `GraphInspector`
  - `paramsFromSummary`
  - `categorySwatch`
- `web/src/components/graph-canvas-edges.tsx`
  - `GraphEdges`
  - `edgePath`
- `web/src/components/graph-canvas-minimap.tsx`
  - `CanvasMinimap`
- `web/src/components/graph-canvas-rendering.ts`
  - `buildNodeMap(nodes)`
  - `buildRunStepStateMap(steps)`
  - `buildEdgeSignatureSet(edges)`
  - `nodeDiffState(node, baseNode, hasProposal)`

### GraphCanvas responsibility after split

`graph-canvas.tsx` keeps:

- props
- view/viewport state
- selection/layout draft state
- canvas pan and node drag event glue
- `drawGraph`, `displayNodes`, `layoutUpdates`
- rendering composition using child components

### Large graph lookup

Add:

```ts
const stepStateByNodeId = useMemo(
  () => buildRunStepStateMap(run.steps),
  [run.steps],
);
```

Then node rendering uses:

```ts
stepStateByNodeId.get(node.id) ?? node.status
```

This replaces the current per-node `run.steps.find(...)`.

### Edge rendering

Move SVG path rendering into `GraphEdges`:

```tsx
<GraphEdges
  edges={drawGraph.edges}
  nodeById={nodeById}
  baseEdgeIds={baseEdgeIds}
  hasProposal={Boolean(pendingProposal)}
/>
```

### Tests

Update `app.test.tsx` to import any newly exported helper directly where needed. Add a large fixture test for `buildRunStepStateMap` and `buildNodeMap` to prove lookup shape without DOM dependence.

## Product Requirement Mapping

| Requirement | Implementation Area | Verification |
| --- | --- | --- |
| PRD-01, PRD-02 | component split files and smaller `graph-canvas.tsx` | `wc -l`; code review |
| PRD-03 | unchanged GraphCanvas composition | `cd web && npm test -- app.test.tsx` |
| PRD-04, PRD-05 | `graph-canvas-rendering.ts` maps/sets | new large fixture helper test |
| PRD-06 | one-way imports from main to children/helpers | `npm run build` |
| PRD-07 | existing and new web tests | `cd web && npm test -- app.test.tsx` |

## Risks

- Moving `paramsFromSummary` can change node height/minimap math if duplicated incorrectly.
- Moving edge path math can break proposal edge diff class.
- Moving inspector can lose click propagation guards and close behavior.
- Helper re-export changes can break existing test imports.
- Over-scoping into viewport culling can change visible behavior; do not do it in this PR.

## Verification Commands

```sh
cd web && npm test -- app.test.tsx
cd web && npm run build
cargo check --workspace
cargo test --workspace
git diff --check
```

## Rollback Plan

- Move split components back into `graph-canvas.tsx`.
- Revert `GraphCanvas` to direct `run.steps.find` lookup.
- Remove new rendering helper tests.
- Existing GH42/GH44 behavior remains recoverable from the prior merged main commit.
