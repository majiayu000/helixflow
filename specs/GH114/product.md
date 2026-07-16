# Product Spec

## Linked Issue

GH-114（#114）

## 用户问题

用户在 Helixflow 聊天输入框发送普通消息后，后端请求能够成功，Agent 回复也已经进入前端状态，但聊天区域没有显示普通用户消息、Agent 回复或一般 system/error 消息。用户看到的结果是“发送后没反应”，无法判断请求是否成功、Agent 回答了什么，或本地 gate 为什么阻止了操作。

现有 ChatPane 仍能显示编辑会话、proposal、RunErrorCard 和部分 tool log，但这些辅助内容不能替代完整的对话时间线。现有测试主要检查消息进入 store，没有验证用户最终能在界面中看到回复，因此回归没有被测试套件发现。

## 目标

- 恢复按时间顺序可见的聊天消息时间线。
- 让普通 user、agent chat 和 system/error 消息都能被用户看到。
- 保留并正确组织现有 tool log、proposal、RunErrorCard 和 edit-session 内容。
- 为“发送后回复最终出现在界面”建立组件级回归测试，而不是只检查 store。
- 保持中文 IME、Shift+Enter、发送中状态和已有工作台行为不回退。

## 非目标

- 不修改后端消息 API、消息持久化 schema 或 Agent runtime。
- 不在本 issue 中实现多轮会话记忆；该问题由 HF-013 单独跟踪。
- 不重新设计整个 Workbench、proposal 或 run/error 视觉系统。
- 不改变 proposal 自动应用、cost gate、run interrupt 或 manual edit session 的产品语义。
- 不新增附件上传、Markdown 富文本、消息编辑、删除或搜索能力。

## Behavior Invariants

1. P1：当 workspace state 包含普通 user 消息时，ChatPane 必须显示该消息正文，且不得因没有 tool log、proposal 或 run error 而隐藏。
2. P2：当 workspace state 包含 `role=agent`、`kind=chat` 的普通 Agent 回复时，ChatPane 必须显示回复正文。
3. P3：当 workspace state 包含一般 system/error 消息时，ChatPane 必须显示可读反馈；禁止只写入 store 而不给用户任何可见结果。
4. P4：消息时间线必须保持后端/状态提供的稳定顺序；同一条消息不得同时以普通消息和 tool log 重复显示。
5. P5：`agent_log:*` 消息继续作为 Agent turn 的辅助运行证据分组展示，不得覆盖或取代该 turn 的普通 Agent 回复。
6. P6：proposal、RunErrorCard 和 edit-session workspace 继续可见，并作为时间线或聊天区域的补充内容存在；本修复不得移除其既有入口。
7. P7：没有任何消息、proposal 或 run error 时，聊天区域保持明确空状态，不生成伪造欢迎消息或 demo 对话。
8. P8：发送请求处于 pending 时，输入与发送控件继续呈现已有 busy/disabled 语义；请求完成后控件恢复可用，回复可见。
9. P9：中文 IME composition 中按 Enter 不得发送；composition 结束后 Enter 发送，Shift+Enter 继续换行。
10. P10：超长正文必须在聊天区域内安全换行或滚动，不得破坏主布局；空正文按空数据处理，不编造占位回复。
11. P11：消息正文按文本安全渲染，不引入未经清洗的 HTML 执行路径。
12. P12：组件级测试必须验证最终 DOM/markup 中出现 user、Agent chat 和 system/error 正文，不能只断言 Zustand state。

## 验收标准

- [ ] 载入包含 user、agent chat、system/error 的 workspace state 后，三类正文均在 ChatPane 中可见且顺序稳定。
- [ ] 一次普通聊天请求成功后，返回的 Agent 回复最终出现在渲染结果中。
- [ ] 一次消息请求失败或被 dirty-edit gate 阻止后，用户能看到明确错误反馈。
- [ ] 带 tool log 的 Agent turn 同时保留普通回答与折叠/分组日志，不重复渲染消息。
- [ ] proposal、RunErrorCard、edit-session summary 的既有渲染测试继续通过。
- [ ] 中文 IME Enter、普通 Enter、Shift+Enter 行为继续通过测试。
- [ ] 空消息状态不显示伪造内容；长文本不会撑破聊天栏。
- [ ] `cd web && npm test` 通过。
- [ ] `cd web && npm run build` 通过。

## 边界情况

- workspace 只有历史 user 消息、尚无 Agent 回复：显示已有 user 消息，不伪造回答。
- Agent 回复为空：保持空数据语义，不显示“成功”等推测性文案。
- system/error 与 RunErrorCard 同时存在：两者各自保持来源语义，不把一般 system 消息误归类为 run error。
- 多个 Agent turn 各自包含 tool log：日志必须归入正确 turn，不跨 turn 混合。
- proposal 已自动应用但仍有历史 proposal 消息：普通对话不得因为 proposal 状态为空而消失。
- event stream 断开：已加载消息仍可阅读；本 issue 不承诺修复事件 catch-up。
- workspace 切换：新 workspace 的消息时间线替换旧 workspace，不残留上一 workspace 内容。

## 发布说明

这是聊天核心可见性回归修复。发布后，用户发送普通消息会在聊天区域看到自己的消息、Agent 回复和一般错误反馈。后端 API、数据库消息格式和 Agent 行为不变，不需要数据迁移。
