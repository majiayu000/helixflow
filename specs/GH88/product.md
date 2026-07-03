# Product Spec

## Linked Issue

GH-88

## 用户问题

React canvas 仍主要从 `WorkbenchState.graph` 渲染，而不是从后端持久化的
`CanvasDocument` 渲染。用户打开 workspace 时看到的是 graph view 迁移前状态，
无法把后续 canvas-agent 编辑建立在后端 source of truth 上。

## 目标

- 前端 hydration 时获取 `/api/workspaces/{workspace_id}/canvas`。
- 在前端状态中保存 canvas `seq`、nodes、edges、comments、runtime 和 metadata。
- `GraphCanvas` 在存在 `CanvasDocument` 时优先从 canvas 渲染。
- 保留 graph-only workspace 的空白 canvas fallback。
- 保持现有 proposal preview 行为。

## 非目标

- 不实现全部 canvas editing ops。
- 不重设计整个 app shell。
- 不移除现有 `WorkbenchState.graph` fallback。

## Behavior Invariants

1. 打开 workspace 会加载后端 canvas snapshot。
2. 有 canvas snapshot 时，canvas 节点和边以 `CanvasDocument` 为渲染源。
3. 只有 graph 数据的旧 workspace 仍能打开，并显示有效空白或迁移 canvas。
4. 空 workspace 不显示 fake demo content。
5. proposal preview 仍可渲染并保持 review/apply/dismiss 语义。

## 验收标准

- [ ] Opening a workspace loads backend canvas snapshot.
- [ ] `GraphCanvas` can render workflow nodes and edges from `CanvasDocument`.
- [ ] Empty graph-only workspaces still render a blank canvas.
- [ ] Existing proposal preview UI still works.
- [ ] TypeScript build passes.

## 边界情况

- canvas fetch 失败时显示错误状态，不静默退回到错误数据。
- snapshot 为空但 graph 存在时，必须保留兼容 fallback。
- proposal preview 不能把 preview state 当成 durable canvas state。

## 发布说明

该变更是 canvas-agent 迁移第一步；后续 issue 负责 durable ops、presence、
run/artifact backfill 和 sync hardening。
