# Workbench UI Refresh Analysis

## 输入材料

设计稿 zip 包含：

- `.thumbnail`
- `Helixflow Workbench.dc.html`
- `support.js`

HTML 原型包含三个设计方向和多个关键状态：

- `1a Ember Studio`: chat 左停靠、经典端口节点图、Agent proposal 审阅。
- `1b Violet Signal`: 画布优先、浮层 chat、运行中。
- `1c Daylight Gallery`: 浅色输出挑选、底部命令栏。
- `2a 导演工作台`: 影棚黑 + Ember，主工作台和 prompt 新旧对照卡。
- `2b Cost Gate`: 花钱前确认，sweep 串行执行，可中断。
- `2c 放映室`: 输出评审，大屏比较和选用。
- `2d 失败诊断`: 失败后 Agent 诊断并生成最小修复 proposal。
- `2e 模板库起点`: 空状态不空，从模板或一句话开始。
- `3a 画布手动编辑`: 框选、手动连线、节点工具条、节点对话框、小地图、撤销重做、提交编辑为新版本。

## 推荐应用策略

第一轮不应该混用三套视觉方向。推荐锁定：

- 主视觉: `2a` 的导演工作台深色 shell。
- 核心交互: `3a` 的手动编辑态和 uncommitted edit lifecycle。
- 状态闭环: `2b`、`2c`、`2d`、`2e`。
- 暂缓: `1b` 浮层 chat、`1c` 浅色输出页作为后续 theme/alternate layout，不进入第一轮。

这样做的原因是现有代码已经是左 chat + 中央 canvas + top bar + docks 的结构，`2a`/`3a` 可以复用现有布局骨架；`1b`/`1c` 会引入更大的导航和响应式重排。

## 当前代码对照

| 设计要求 | 当前位置 | 当前能力 | 差距 |
| --- | --- | --- | --- |
| 导演工作台 shell | `web/src/app.tsx`, `web/src/styles.css`, `web/src/panels.css` | 已有 `TopBar`、左 chat、中央 canvas、运行/输出浮层 | 视觉层级、品牌、状态信息和 cost/version 展示需要重组 |
| 工作区/版本/运行状态 top bar | `web/src/components/top-bar.tsx` | 有 workspace tab、provider、connection、undo/history/export/run | 仍显示 `ComfyUI Agent`，缺 daily cost、editing dirty badge、Queue lock reason |
| Proposal 审阅 | `web/src/components/chat-pane.tsx`, `GraphCanvas` | `pendingProposal` 能 preview graph，chat 里有 proposal card | 缺 prompt old/new 对照、diff 作为主审阅卡、Queue locked by proposal 的一致表达 |
| 手动编辑态 | `web/src/components/graph-canvas.tsx` | 有 view/edit/review mode、框选、多选、拖动节点、布局草稿保存 | 只保存 layout；没有批量 uncommitted ops、commit to version、discard session |
| 小地图/缩放 | `graph-canvas-navigation.ts`, `graph-canvas-minimap.tsx` | 已有 minimap、zoom、pan、fit view 等基础 | 需要和新视觉整合，不是核心技术缺口 |
| 节点工具条/节点对话框 | `GraphInspector`, `ManualProposalPanel` | Inspector 只读；ManualProposalPanel 是表单式 proposal 生成器 | 需要内联工具条、节点上下文对话、schema 驱动参数编辑 |
| 手动连线/新增节点 | `ManualProposalPanel`, `manual-proposal-helpers.ts` | 可通过表单创建 add_node/add_edge proposal | 设计要求在 canvas 直接拖端口、空白处释放建节点 |
| Cost Gate | `ConfirmModal`, `RunDock` | 已有 `pendingConfirmation`、confirm/hold、run 状态 | 需要更强的 cost/runCount/interruptibility UI，且必须在花钱前确认 |
| 输出评审 | `ArtifactStage`, `OutputsStrip` | 已有 artifact preview 和 output select | 需要支持放映室模式、大图比较、Agent 推荐、选用后状态 |
| 失败诊断 | `RunErrorCard`, `run-panels.tsx` | 失败卡能展示结构化错误和 raw error | 需要从失败节点直接进入最小修复 proposal 的闭环 |
| 模板库起点 | `LoadingShell`, empty chat/canvas | 空 workspace 不再伪造 demo graph | 需要真实模板入口，仍不能恢复 fake demo content |

## 产品语义差距

最大缺口是“编辑生命周期”：

- 现在节点拖动产生的是 layout draft，保存后走 version layout API。
- 手动 graph op 现在通过独立 `ManualProposalPanel` 生成 proposal。
- 设计稿要求用户手动编辑和 Agent proposal 共存，但手动编辑必须先累积为 `UNCOMMITTED`，再提交成 `source=user` 的新版本。
- Queue 在有未提交编辑或 pending proposal 时必须明确锁住，不能静默以旧版本运行。

因此，应用新 UI 的核心不是重写 CSS，而是新增一个 workbench edit session 概念，并把 layout move、set_param、add_edge、add_node、remove_node、copy/paste/delete 都收敛到同一批可提交 ops。

## 分阶段落地

### Phase 0: 设计证据和 UI token

- 保留设计源文件和截图到 `artifacts/ui-design/helixflow-workbench-20260702/`。
- 将颜色、字体、间距、radius、shadow 整理成 CSS variables。
- 不改变产品行为，只使后续组件改造有统一 token。

### Phase 1: Director shell

- 将品牌从 `ComfyUI Agent` 收敛为 `helixflow`。
- Top bar 展示 workspace name、version、provider、connection、node count、dirty/editing badge、Queue disabled reason。
- 保留现有左 chat、中央 canvas、history、confirm modal、run dock、outputs strip。

### Phase 2: Manual edit session

- 在 store 中引入 `editingSession`。
- Canvas 上的 move、set_param、add_edge、add_node、remove_node 先进入 `dirtyOps`。
- 有 dirty ops 时 Queue 禁用，显示 `EDITING · N CHANGES`。
- Commit 调用后端创建新 version，返回最新 `WorkbenchState`。
- Discard 清空 dirty ops 并恢复 committed state。

### Phase 3: Canvas native controls

- 增加 canvas tool mode: select、pan、connect、text/image/video node、import、asset library。
- 增加 selection floating toolbar: copy、delete、align、generate from selection。
- 增加 node toolbar: edit text、font controls、generate、node chat。
- 增加 node dialog: 以选中节点和上游上下文为输入，结果回填为 op/proposal。
- 任何 catalog/schema 请求失败时 fail closed：禁用新增/编辑控件并显示错误，不能 fallback 展示全部节点类型。

### Phase 4: Run, cost, output, failure states

- Cost gate 作为花钱前唯一入口。
- Output review 强化为 `2c` 放映室模式，但复用 `ArtifactStage` / `OutputsStrip` 数据流。
- 失败诊断从 `RunErrorCard` 进入最小修复 proposal，而不是只展示错误。

### Phase 5: Template empty state

- 空 workspace 显示模板库和 prompt start。
- 模板只能生成真实 proposal 或 version，不能恢复 fake demo graph。

## 推荐拆分

第一批实现不要一次吞下完整 UI。推荐顺序：

1. Shell + visual tokens + top bar state wording。
2. Edit session model + Queue lock + commit/discard。
3. Node/canvas native controls。
4. Run/cost/output/failure state refinement。
5. Template empty state。

如果走 PR 拆分，Phase 2 是最关键的行为 PR；没有它，设计稿里的 `3a` 只能变成静态皮肤。
