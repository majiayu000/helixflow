# Product Spec

## Linked Issue

GH72。

## 用户问题

Helixflow 当前工作台已经能通过聊天生成 workflow、审阅 proposal、确认 run、查看 outputs，并有基础 canvas 导航。但新 UI 设计要求的是更完整的导演工作台：用户能在同一个画布中审阅 Agent 提案、手动编辑节点图、提交自己的版本、确认花费、评审输出、从失败中进入最小修复闭环。

如果只替换样式，用户仍会遇到这些问题：

- 手动编辑、Agent proposal、run queue 的状态边界不清楚。
- 未提交编辑可能和运行或 Agent proposal 发生版本语义冲突。
- 节点编辑、连线、新增节点仍被迫走旁边的表单面板，不像真正 canvas editor。
- 花钱运行、失败诊断、输出选择没有被组织成同一个清晰工作流。

## 目标

- 把新设计应用为 Helixflow 的主 Workbench shell，而不是一次性替换所有页面。
- 保持真实 workspace / version / proposal / run / artifact 数据流，不恢复 fake demo content。
- 引入用户可理解的手动编辑生命周期：`UNCOMMITTED` -> commit as new version -> Agent/run uses latest committed version。
- 在 UI 中明确 Queue 禁用原因：pending proposal、pending confirmation、active run、provider unavailable、dirty edits。
- 让节点编辑从表单式 side panel 逐步迁移到 canvas-native toolbar/dialog。
- 保留花钱前确认、人类 gate、失败可诊断、输出可选择的闭环。

## 非目标

- 不在本 spec 中实现完整多人协作 presence。
- 不把 graph editor 替换为大型第三方 canvas 库。
- 不实现新的 provider、BYOK、计费后端或真实外部模型调用。
- 不改变 SpecRail human gates，不自动创建 issue、PR、label 或 merge。
- 不把 `1b` 浮层 chat 和 `1c` 浅色 gallery 作为第一轮主布局。

## Behavior Invariants

1. P1: Workbench 首屏必须加载真实 workspace state；没有 workspace graph 时显示真实空状态或模板入口，不显示伪造 demo graph。
2. P2: Top bar 必须展示 workspace name、current version、provider/connection 状态、node count、run action、history/export/undo，以及当前禁用 Queue 的主要原因。
3. P3: 当存在 `pendingProposal` 时，canvas 显示 proposal preview，用户可以 apply 或 dismiss；Queue 必须锁住并说明原因。
4. P4: 当用户在 canvas 进入手动编辑态时，move_node、set_param、add_edge、add_node、remove_node 等操作先进入 uncommitted edit session，不得直接伪装为已提交版本。
5. P5: 有 uncommitted edits 时，Top bar 显示 `EDITING · N CHANGES`，left chat/session area 显示可读 change summary，并提供 commit/discard。
6. P6: 有 uncommitted edits 时，Queue 必须禁用；用户提交为新 version 后，Agent 和 run 才能基于该 version 继续。
7. P7: Commit manual edits 必须创建新的 version，history 中能区分 `source=user`；discard 不得改变 committed graph。
8. P8: Canvas tools 至少覆盖 select、pan、connect、text/image/video node add、import、asset library entry；暂未支持的能力必须 disabled with reason，不能静默隐藏或 fallback。
9. P9: Node toolbar/dialog 必须围绕选中节点工作；发送给 Agent 的上下文只能包含声明过的 selection/upstream fields，unknown fields 必须被 schema 拒绝。
10. P10: 任何付费或真实 provider run 前必须出现 cost gate；用户确认前不得执行花费动作。
11. P11: Run outputs 必须可比较、可选择、可回到 workspace state；选择结果不能只存在于前端临时状态。
12. P12: Run failure 必须显示结构化错误摘要，并提供进入最小修复 proposal 的路径；缺少错误数据时显示 blank/unknown，不编造诊断。
13. P13: Node catalog 或 schema 加载失败时，新增/编辑节点控件必须 fail closed 并显示错误；不能 warning 后展示全部节点类型。

## 验收标准

- [ ] 用户打开已有 workspace，能看到符合导演工作台布局的 top bar、chat、canvas、run/output/history 区域。
- [ ] 有 pending proposal 时，Queue 被锁住，proposal preview 和 apply/dismiss 可见。
- [ ] 用户移动节点或编辑参数后，UI 显示 `EDITING · N CHANGES`，Queue 被锁住，change summary 可见。
- [ ] Commit 后生成新 version，dirty state 清空，history 出现 user-sourced version。
- [ ] Discard 后 graph 回到 committed state，不产生新 version。
- [ ] Canvas selection toolbar 能对选中节点展示批量操作入口； unsupported 操作有明确 disabled reason。
- [ ] 节点 dialog 能围绕选中节点发起上下文请求，返回结果以 proposal 或 manual op 进入审阅/提交流程。
- [ ] Cost gate 在 run 前出现，包含 run count、estimated cost、interruptible/serial semantics。
- [ ] Output review 能选择 artifact，并在刷新后保持选择状态。
- [ ] Failed run 能从错误卡进入最小修复 proposal。
- [ ] `cd web && npm test -- app.test.tsx` 覆盖关键 UI 状态。
- [ ] `cd web && npm run build` 通过。

## 边界情况

- Empty workspace: 显示模板入口或 prompt start，不生成假节点。
- Provider unavailable: run controls disabled，并展示 provider status message。
- Pending confirmation: 用户未确认前不能 run。
- Active run: Queue button 切换为 interrupt，不能重复提交 run。
- Pending proposal + manual edit: 不允许同时 commit manual edits 和 apply proposal；UI 必须要求用户先处理其中一种状态。
- Dirty edits + workspace switch: 切换前必须提示 commit/discard，或者自动阻止切换并说明原因。
- Catalog request failure: 新增节点和 schema-driven edit 控件禁用并显示错误。
- Offline/event stream disconnected: 已加载的 committed state 可以浏览，但 commit/run/Agent actions 必须显示不可用原因。
- IME input: Chat/node dialog 的 Enter 行为必须继续尊重中文输入法 composition 状态。

## 发布说明

该改版会改变 Workbench 的主视觉和交互顺序。发布时需要说明：

- 手动编辑现在先进入 uncommitted edit session。
- Queue 在 dirty edits 或 pending proposal 时会锁住。
- 花钱运行仍需要用户确认。
- 旧的 `ManualProposalPanel` 若保留，应标为 advanced/debug 路径，而不是主编辑入口。
