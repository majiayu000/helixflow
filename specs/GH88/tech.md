# Tech Spec

## Linked Issue

GH-88

## Product Spec

Link to `product.md`.

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Frontend types/API | `web/src/types.ts`, `web/src/api.ts` | Typed workbench and canvas API boundaries exist. | Canvas snapshot must cross this boundary without ad hoc parsing. |
| Store | `web/src/store.ts` | Workbench state is graph-centered. | Needs a canvas slice and loading/error status. |
| App hydration | `web/src/app.tsx` | Workspace hydration loads workbench state. | Must also load canvas snapshot. |
| Canvas render | `web/src/components/graph-canvas.tsx` | Canvas render path is primarily graph-derived. | Must prefer `CanvasDocument` when present. |
| Tests | `web/src/app.test.tsx` | Existing UI tests cover app/proposal behavior. | Must prove fallback and preview compatibility. |

## 设计方案

Add a frontend canvas slice with `canvasStatus`, `canvasError`, `canvas`,
`canvasConnection`, selected ids, and presence map. Extend hydration to fetch the
workspace canvas snapshot. Keep `WorkbenchState.graph` available while
`GraphCanvas` chooses `CanvasDocument` as source of truth when the snapshot is
available.

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 | `web/src/api.ts`, `web/src/app.tsx` | `npm test` |
| P2 | `web/src/components/graph-canvas.tsx` | `npm test` |
| P3 | `web/src/store.ts`, fallback conversion | `npm test` |
| P5 | proposal preview render path | `npm test` |

## 数据流

Input: workspace id from current app state. Output: canvas snapshot stored in
frontend state. Persistence remains backend-owned through existing canvas APIs.

## 备选方案

- Render from graph only until all canvas ops exist: rejected because later
  durable editing would still target the wrong source of truth.

## 风险

- Security: no new secrets or auth behavior in this issue.
- Compatibility: graph-only fallback must remain.
- Performance: snapshot load should not block unrelated shell rendering longer
  than existing workspace hydration.
- Maintenance: avoid duplicating graph-to-canvas mapping in multiple components.

## 测试计划

- [ ] Unit tests: canvas snapshot parsing/rendering.
- [ ] Integration tests: workspace hydration fallback and proposal preview.
- [ ] Manual verification: open existing workspace and empty workspace.

## 回滚方案

Disable canvas-priority rendering and keep graph fallback while preserving API
types.
