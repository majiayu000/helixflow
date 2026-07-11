# Task Plan

## Linked Issue

GH-110 (#110)

## Spec Packet

- Product: `specs/GH110/product.md`
- Tech: `specs/GH110/tech.md`

## Implementation Tasks

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP110-T1 | frontend | none | 提取 canvas connection controller | 连线/断线 handler 复用现有规则，GraphCanvas 行为不变 | connection/editing tests |
| SP110-T2 | frontend | none | 提取 canvas node drag controller | move capability、draft 和 proposal 生命周期不变 | editing tests |
| SP110-T3 | frontend | none | 提取 App navigation/action hook | 所有导航入口和 dirty 决策继续复用现有 state machine | App navigation tests |
| SP110-T4 | qa | T1-T3 | 全量回归与文件尺寸验证 | 核心文件低于阈值，无新增超大文件 | Web tests/build；`wc -l` |
| SP110-T5 | qa | T1-T4 | 后端与 spec 回归 | Rust 和 SpecRail 全绿 | `cargo test --workspace`; workflow checks |

## PR Split / Ownership

- 单一 PR：纯结构拆分，串行编辑 `graph-canvas.tsx` 和 `app.tsx`。
- 新文件：两个 canvas controller 与一个 App navigation hook。
- 不修改 API、store schema、CSS 或后端行为。

## Verification

- `cd web && npm test`
- `cd web && npm run build`
- `cargo check --workspace`
- `cargo test --workspace`
- `python3 checks/check_workflow.py --repo . --spec-dir specs/GH110`
- `python3 checks/check_workflow.py --repo . --all-specs`
- `wc -l web/src/components/graph-canvas.tsx web/src/app.tsx web/src/components/graph-canvas-connection-controller.ts web/src/components/graph-canvas-node-drag-controller.ts web/src/use-workbench-navigation.ts`

## Handoff Notes

- 本 tranche 只重构，不改变已有 GH105 行为契约。
- review 必须以 exact PR head 运行既有 React integration tests。
