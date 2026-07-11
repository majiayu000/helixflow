# Product Spec

## Linked Issue

GH-110 (#110)

## 用户问题

Workbench 的行为完整性已经有回归测试保护，但核心前端编排文件继续膨胀：`graph-canvas.tsx` 已达到 800 行硬上限，`app.tsx` 为 506 行。画布交互、连线、节点拖动与渲染集中在一个组件内，应用壳层也同时承担 workspace 列表、dirty navigation 和通用 action 生命周期，后续修改难以局部审查。

## 目标

- 按现有职责边界拆分画布连线与节点拖动控制器。
- 把 App 的 workspace/action/navigation 编排提取为聚焦 hook。
- 保持所有 props、API、状态模型和用户可见行为不变。
- 降低核心文件尺寸，使后续改动能在更小边界内测试和审查。

## 非目标

- 不重做 UI、CSS、画布数据模型或 store。
- 不新增功能、接口、字段、快捷键或导航决策。
- 不改变 view/review/edit capability 语义。
- 不为了拆分删除、跳过或弱化测试。

## Behavior Invariants

1. view mode 仍只允许 select、pan、zoom，所有 mutation 入口继续 fail closed。
2. pending proposal/review mode 期间不得完成迟到的 move、resize 或 connection。
3. dirty navigation 的 Commit / Discard / Cancel、pending 去重和失败保留行为不变。
4. `runAction` 继续统一 busy 生命周期并在成功后刷新 workspace 列表。
5. workspace switch、undo、restore、create 和 URL 更新顺序不变。
6. 所有既有 GraphCanvas 与 App props 保持兼容。

## 验收标准

- AC1：`web/src/components/graph-canvas.tsx` 少于 650 行。
- AC2：`web/src/app.tsx` 少于 400 行。
- AC3：新增 production 文件均不超过 650 行。
- AC4：Web 全量测试和 production build 通过。
- AC5：Rust workspace tests 与 SpecRail checks 通过。
- AC6：独立审查确认 dirty navigation 和 canvas capability 无行为漂移。

## 发布说明

内部前端结构优化：Workbench 行为不变，画布交互和应用导航编排现在位于更小、更聚焦的模块中。
