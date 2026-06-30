# GraphCanvas Viewport Persistence, Wheel Zoom, And Minimap

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/42
Locale: zh-CN

## 背景

Helixflow 的工作台已经能通过聊天生成 workflow、预览 pending proposal、确认 run，并展示 outputs。当前 `GraphCanvas` 主要是 review canvas：用户可以平移、点按钮缩放、选择节点并查看 inspector，但在节点较多或 graph 较大时，画布导航仍不够稳定。

对比 `basketikun/infinite-canvas` 后，第一步应先补齐低风险的画布导航体验，而不是立即引入节点编辑、连线编辑或后端 graph schema 变化。

## 目标

1. 用户可以用滚轮围绕鼠标位置缩放 `GraphCanvas`。
2. 每个 workspace 记住自己的最近 viewport，刷新或重新打开后恢复。
3. 用户可以通过 minimap 看见 graph 分布和当前视口位置。
4. 用户可以点击或拖动 minimap 快速移动主画布视口。
5. Pending proposal preview 模式继续显示 preview graph，并保留新增/修改 diff 高亮。

## 非目标

- 不实现节点拖拽持久化。
- 不实现手动新增、删除或连接节点。
- 不改变后端 `WorkflowGraph` schema。
- 不改变 proposal apply/dismiss、run execution、cost gate 或 artifact preview 行为。
- 不引入大型第三方 canvas/graph editor 库替换当前 DOM/SVG canvas。
- 不把 viewport 写入后端 store；第一版只做浏览器本地持久化。

## 用户场景

### 场景 1：查看较大 workflow

用户打开一个节点较多的 workspace 后，可以用滚轮缩放并拖动画布。缩放时鼠标下的 graph 区域保持在鼠标附近，用户不会因为缩放丢失正在看的节点。

### 场景 2：恢复上次查看位置

用户在 workspace A 中平移或缩放到某个区域后刷新页面，画布恢复到该 workspace 的最后 viewport。切换到 workspace B 时，workspace A 的 viewport 不会污染 workspace B。

### 场景 3：通过 minimap 快速定位

当 graph 有节点时，minimap 显示节点分布和当前视口矩形。用户点击或拖动 minimap 后，主画布移动到对应 graph 区域。

### 场景 4：审阅 pending proposal

当存在 pending proposal 时，主画布和 minimap 都基于 preview graph 显示；新增/修改节点仍然保留已有 diff 高亮，用户可以在审阅时导航整个 preview graph。

## 产品需求

| ID | Requirement |
| --- | --- |
| PRD-01 | `GraphCanvas` 必须支持 wheel zoom，且缩放围绕鼠标位置锚定。 |
| PRD-02 | `GraphCanvas` viewport 必须有明确上下界，避免缩放到不可用状态。 |
| PRD-03 | Viewport persistence 必须按 workspace id 隔离。 |
| PRD-04 | Viewport persistence 必须只影响浏览器本地体验，不改变 server-owned graph。 |
| PRD-05 | Minimap 必须显示所有可见 graph 节点的相对分布。 |
| PRD-06 | Minimap 必须显示当前主画布视口矩形。 |
| PRD-07 | 点击或拖动 minimap 必须更新主画布 viewport。 |
| PRD-08 | 空 graph、无 pending proposal、或节点坐标异常时 UI 不得崩溃。 |
| PRD-09 | Pending proposal preview 仍基于 `pendingProposal.previewGraph` 渲染，并保留 diff 高亮。 |

## 验收标准

- 在 `GraphCanvas` 上触发 wheel event 后，zoom 值改变，鼠标下 world 坐标保持稳定。
- Zoom 被限制在可用范围内。
- 同一个 workspace 刷新后恢复最近 viewport。
- 切换 workspace 后使用对应 workspace 的 viewport，不复用前一个 workspace 的 viewport。
- 有节点时 minimap 显示节点分布和视口矩形。
- 点击或拖动 minimap 会移动主画布视口。
- 空 graph 仍显示空画布状态，minimap 不报错。
- Pending proposal preview 的节点和连线仍来自 preview graph，diff class 仍保留。
- 前端测试覆盖 wheel zoom anchor、workspace-scoped viewport restore、minimap navigation、pending proposal preview 不回退。

## 开放问题

1. 未来是否需要把 viewport 持久化到后端，以便跨浏览器恢复？
2. 下一步是否应在独立 issue 中实现节点拖拽持久化？
