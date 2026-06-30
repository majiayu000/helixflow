# Versioned GraphCanvas Node Repositioning

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/44
Locale: zh-CN

## 背景

GH42 已补齐 `GraphCanvas` 的 viewport persistence、wheel zoom 和 minimap。用户现在可以稳定浏览较大的 workflow，但节点位置仍只能由 graph 生成结果决定；用户无法整理布局，也无法把整理后的布局保存成 server-owned current graph。

对比 `basketikun/infinite-canvas` 后，下一步应补齐低风险的节点重排能力：允许用户拖动节点、临时预览位置、显式保存布局，并通过现有 version/history 机制追踪变化。

## 目标

1. 用户可以拖动单个节点调整位置。
2. 用户可以多选节点，并把选中的节点组一起拖动。
3. 拖动时连线路径跟随节点位置实时更新。
4. 拖动只产生浏览器内临时布局，直到用户点击保存。
5. 保存布局后，后端创建新的 current graph version。
6. 刷新 workspace 后，节点位置来自 server-owned current graph。
7. Pending proposal preview 模式下不允许直接保存布局。

## 非目标

- 不新增或删除节点。
- 不新增或删除连线。
- 不编辑 node params。
- 不改变 run execution 语义。
- 不改变 proposal apply/dismiss 语义。
- 不新增框选、快捷键、剪贴板能力；这些属于 GH45。
- 不实现手工 add/remove/connect/edit graph proposal；这些属于 GH46。
- 不引入第三方 canvas/graph editor 库替换当前 DOM/SVG 实现。

## 用户场景

### 场景 1：整理单个节点

用户打开已有 workspace 后，可以拖动一个节点到更清晰的位置。拖动期间边线跟随节点移动，但刷新前如果没有保存，server-owned graph 不会被修改。

### 场景 2：整理一组节点

用户通过已有选择交互扩展选中多个节点后，拖动其中一个选中节点时，整组节点按相同偏移移动。

### 场景 3：保存布局

用户完成节点重排后，点击保存布局。保存成功后 UI 显示新的 current version，刷新 workspace 后节点位置保持不变，history 中出现可追踪的 layout update。

### 场景 4：审阅 pending proposal

当存在 pending proposal 时，画布继续显示 preview graph，但用户不能把 preview 布局直接保存为 current graph，避免覆盖尚未 apply/dismiss 的 proposal。

## 产品需求

| ID | Requirement |
| --- | --- |
| PRD-01 | `GraphCanvas` 必须支持拖动当前 graph 节点。 |
| PRD-02 | 节点拖动必须先进入临时 dirty layout，不得直接修改 server-owned graph。 |
| PRD-03 | 拖动时节点位置和相关 edge path 必须实时更新。 |
| PRD-04 | 多选节点拖动时，所有选中节点必须按同一偏移移动。 |
| PRD-05 | Dirty layout 必须提供明确的保存入口和未保存状态提示。 |
| PRD-06 | 保存布局必须使用当前 workspace version id 作为 baseVersionId。 |
| PRD-07 | 保存成功后必须刷新 `WorkbenchState`，并清空 dirty layout。 |
| PRD-08 | 保存布局必须创建可追踪的 version/history 记录。 |
| PRD-09 | 后端必须拒绝未知节点 id、非有限坐标、stale baseVersionId。 |
| PRD-10 | Pending proposal 存在时，前端不得提供保存入口，后端也必须拒绝保存。 |

## 验收标准

- 用户拖动单个节点时，节点和连接该节点的 edge path 同步移动。
- 用户多选节点后拖动其中一个选中节点，所有选中节点一起移动。
- 节点拖动后出现未保存提示和保存布局入口。
- 保存布局成功后，workspace current version id 改变，history 出现 layout update。
- 保存布局成功后刷新同一 workspace，节点位置仍为保存后的位置。
- 保存布局请求包含 `baseVersionId`，后端在当前 version 已变化时返回 conflict。
- 保存布局请求包含未知 node id 或非有限坐标时，后端返回错误且 current version 不变。
- Pending proposal preview 下不能保存布局；直接调用 API 也返回 conflict。
- 前端测试覆盖单节点移动、多节点移动、dirty/save 状态、pending proposal guard。
- 后端测试覆盖成功保存、stale baseVersionId、未知节点、pending proposal guard。

## 开放问题

1. 保存布局是否需要在未来合并为 proposal op，而不是直接 manual version？
2. 多选交互在 GH45 完成前是否需要临时支持 modifier-click？
