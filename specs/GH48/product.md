# Split GraphCanvas Components And Improve Large-Graph Rendering

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/48
Locale: zh-CN

## 背景

GH42 和 GH44 已让 `GraphCanvas` 具备 viewport/minimap 导航、节点拖拽和版本化布局保存。当前主组件仍承担 toolbar、edge、node、inspector、minimap、drag state 和 helper glue，文件体积偏大，后续 GH45/GH46 继续添加选择、快捷键、手工编辑时会增加维护风险。

#48 的目标是先建立清晰组件边界，并补上基础大图渲染保护，不改变用户可见交互。

## 目标

1. `GraphCanvas` 主文件回到可维护大小。
2. Minimap、node card、inspector、edge SVG、toolbar/render helper 有清晰模块边界。
3. 现有 viewport、minimap、proposal preview、run status、node drag/save 行为不回退。
4. 大 graph 渲染避免明显 O(n²) 查找热点。
5. 新增或保留测试覆盖拆分后的关键行为。

## 非目标

- 不新增用户可见交互功能。
- 不改变 graph schema、store schema 或 HTTP API。
- 不改变节点拖拽、布局保存、proposal apply/dismiss、run execution 语义。
- 不实现 GH45 的框选、快捷键、剪贴板。
- 不实现 GH46 的手工 add/remove/connect/edit graph proposal。
- 不引入大型第三方 canvas/graph editor 库。

## 用户场景

### 场景 1：普通工作台行为不变

用户打开 workspace 后，继续能缩放、平移、看 minimap、选择节点、拖动布局并保存；拆分组件不改变现有交互。

### 场景 2：审阅 pending proposal

用户看到 pending proposal preview 时，新增/修改节点高亮、minimap 和布局保存锁定状态保持不变。

### 场景 3：较大 workflow 渲染

当 graph 节点和 run steps 数量增加时，渲染应通过预先建立 map/set 避免对每个节点重复扫描所有 run steps 或边。

## 产品需求

| ID | Requirement |
| --- | --- |
| PRD-01 | `GraphCanvas` 主文件必须只保留 canvas orchestration、state 和事件 glue。 |
| PRD-02 | Node card、inspector、minimap、edges、render lookup helper 必须拆分为独立模块。 |
| PRD-03 | 拆分后现有 viewport/minimap/proposal/run/layout 行为必须保持。 |
| PRD-04 | 渲染节点时不得对每个 node 重复线性查找 run steps。 |
| PRD-05 | Edge 和 node lookup 必须使用 memoized map/set 或等价结构。 |
| PRD-06 | 新模块不得产生循环依赖。 |
| PRD-07 | 现有测试必须继续通过，并新增大 graph lookup 保护测试。 |

## 验收标准

- `web/src/components/graph-canvas.tsx` 回到可维护大小，低于 400 行目标线。
- `GraphCanvas` 拆分后仍渲染 toolbar、node、edge、minimap、inspector。
- Pending proposal preview 仍显示 diff styling 和 locked layout 状态。
- Layout helper、navigation helper 和保存 API 测试继续通过。
- 大 graph fixture 的 run step lookup 使用一次性 map，不使用 `nodes.map(... run.steps.find(...))` 模式。
- `cd web && npm test -- app.test.tsx` 通过。
- `cd web && npm run build` 通过。

## 开放问题

1. 未来是否需要真正 viewport culling，只渲染视口附近节点？
2. GH45 增加框选后，selection state 是否应再抽成独立 hook？
