# Product Spec

## Linked Issue

GH-59

## 用户问题

图编辑存在两套 op 模型:在用的 `ProposalOp`(crates/graph/src/lib.rs:367)与 GH47 定义但零消费方的 `CanvasOpsRequest`/`CanvasOp`(crates/agent/src/canvas_ops.rs:101-116)。双轨增加维护成本且容易漂移。

同时手动编辑体验割裂:`POST /api/workspaces/{workspace_id}/proposals/manual` 每次只能提交一个 op(单枚举 `ManualProposalOpRequest`),且手动编辑还要走 proposal → apply 二次确认。用户改一个参数、连一条线都要两步操作,而 GH44 的 layout 保存已经证明"手动编辑即时成版"是可行且更顺手的模式。

产品决策(owner 已确认,不再讨论):

- 手动直接编辑(改参数/连线/增删节点)跳过 proposal 二次确认,提交即生成新版本(source=manual),靠版本历史 undo/restore 兜底。
- agent 提案保持现有 review 流程(pending proposal → apply/dismiss)不变。
- 本 issue 只交付 API 与模型统一;画布交互 UI 是后续 issue #64/#65/#66。

## 目标

- 单一 op 模型:所有图编辑统一到 `ProposalOp`,删除未接线的 `CanvasOpsRequest`/`CanvasOp` 输出通道。
- 新增手动批量编辑端点:一次请求提交多个 op,原子校验(全成全败),直接生成新版本(source=manual),无 pending proposal。
- 删除手动编辑的 proposal 双轨:移除 `POST /api/workspaces/{workspace_id}/proposals/manual` 端点,前端手动编辑面板改调新端点(不做向后兼容)。
- agent 提案路径行为不变。

## 非目标

- 画布交互 UI(拖拽连线、inspector、节点库)——后续 issue #64/#65/#66。
- CRDT / 实时协作 / 多人编辑合并——只做基于 base version 的简单乐观锁。
- `CanvasOpsContext`(agent 输入上下文,写入 ctx/canvas_state.json)与 `CanvasOpsContract`/`CanvasOpSpec`(写入 ctx/canvas_ops.json 的 agent 契约文件)的改动——它们已接线,保留。
- 版本历史 UI 或版本清理策略的改动。

## Behavior Invariants

1. 手动批量编辑一次请求携带 N(N ≥ 1)个 op,全部合法时恰好生成一个新版本,版本 source=manual、parent 为提交时的 base version;不产生任何 pending proposal,也不产生 proposal 消息。
2. 批量 op 中任一 op 非法(单 op 转换失败、应用该 op 失败,或全部应用后整体图校验失败,如缺失节点、端口类型不匹配、成环),整个请求被拒绝:不生成版本、不落图文件、当前版本指针不动;响应为 HTTP 400,JSON 体包含人类可读的 `error` 消息,单 op 级失败(转换或应用失败)时额外包含定位失败 op 的 `opIndex`(0 起),仅全部应用后的图级校验失败时 `opIndex` 为 null。
3. 并发编辑:请求必须携带 `baseVersionId`;当它不等于工作区当前版本时返回 HTTP 409 且不产生任何写入。两个客户端基于同一版本同时提交时,恰好一个成功,另一个收到 409,刷新后可重试。
4. 工作区存在 pending agent proposal 时,手动批量编辑返回 HTTP 409,proposal 保持 pending 状态不变。
5. 手动批量编辑生成的版本可通过既有 undo(`POST /versions/undo`)与 restore(`POST /versions/{version_id}/restore`)恢复到编辑前状态,恢复版本 source=restore。
6. agent 提案路径行为完全不变:agent 产出的 proposal 仍进入 pending 状态,须经 apply/dismiss;agent 无法通过任何路径绕过 review 直接成版。
7. 空 op 数组、空 `baseVersionId`、未知字段的请求返回 HTTP 400,不产生任何写入。
8. `CanvasOpsRequest`/`CanvasOp`/`CanvasLayoutMove` 删除后,agent 会话的输入上下文文件(ctx/canvas_state.json、ctx/canvas_ops.json)内容与生成行为不变。
9. 既有 layout 保存端点(`POST /versions/layout`)行为不变。

## 验收标准

- [ ] 一次请求提交多个 op(含 add_node/remove_node/set_param/add_edge/remove_edge/move_node),成功生成单个 source=manual 版本;校验失败整体拒绝并返回含 `opIndex` 的结构化错误。
- [ ] 手动批量编辑直接产生新版本,无 pending proposal;版本历史可 undo/restore 恢复。
- [ ] `POST /api/workspaces/{workspace_id}/proposals/manual` 端点及 `manual_proposal_routes.rs` 删除,前端手动编辑面板走新端点且 `web` 测试通过。
- [ ] `CanvasOpsRequest`/`CanvasOp`/`CanvasLayoutMove` 死代码删除,`rg CanvasOpsRequest crates web`、`rg "CanvasOp\\b" crates web`、`rg CanvasLayoutMove crates web` 无任何引用(含测试;限定实现路径,specs/ 历史规范文档中的文字提及不计)。
- [ ] `cargo test --workspace` 与 `cd web && npm test` 全部通过。

## 边界情况

- 单 op 请求:等价于原手动编辑,但即时成版(不再有确认步)。
- 同一请求内 op 之间存在顺序依赖(先 add_node 再 add_edge 指向它):按数组顺序依次应用,依赖成立即合法。
- 同一请求内 op 互相冲突(remove_node 后又对该节点 set_param):应用到该 op 时失败,整体拒绝并返回该 op 的 `opIndex`。
- `move_node` 指向不存在的节点、位置含 NaN/Infinity:400 拒绝。
- `set_param` 对同一节点同一 key 重复设置:第二个 `set_param` 属于应用到该 op 时失败,整体拒绝并返回该 op 的 `opIndex`;只有全部 op 应用完成后才发现的整体图问题使用 `opIndex: null`。
- 请求成功但客户端超时未收到响应:客户端用旧 `baseVersionId` 重试会收到 409,刷新工作区状态即可发现版本已前进,不会重复成版。

## 发布说明

- 破坏性 API 变更:删除 `POST /api/workspaces/{workspace_id}/proposals/manual`,新增 `POST /api/workspaces/{workspace_id}/versions/ops`。本仓库为单体应用,前后端同 PR 内切换,不保留兼容层。
- 无数据库 schema 迁移:versions 表的 source=manual 语义沿用 GH44 先例。
- 已存在的历史 manual proposal 记录不受影响(仍可在历史中查看)。
