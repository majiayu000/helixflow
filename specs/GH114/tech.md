# Tech Spec

## Linked Issue

GH-114（#114）

## Product Spec

[`specs/GH114/product.md`](./product.md)

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| ChatPane 组合 | `web/src/components/chat-pane.tsx` | 主滚动区只挂载 `EditSessionWorkspace`、`RunErrorCard`、`ProposalMessage`，不遍历普通 messages | HF-001 的直接渲染断点 |
| 消息归组 | `web/src/components/chat-pane.tsx` 的 `chatEntries`、`flushPendingLogs` | 已能把 user/system 归为 message，把 Agent reply 与 `agent_log:*` 归为 assistant turn | 应复用现有正确归组逻辑，避免新建平行模型 |
| 消息组件 | `MessageRow`、`AssistantTurn`、`ToolCallGroup` | 组件已实现，但 `MessageRow` 无调用点，`AssistantTurn` 只在 edit-session 内显示最后两个带日志 turn | 修复应接通已有组件，不新增重复 renderer |
| Edit session | `EditSessionWorkspace` | 重复显示 latest user text，并自行筛选最后两个 tool-log entries | 接通完整时间线后会重复消息/日志，需要收敛职责 |
| 消息 schema | `web/src/types.ts` | `ChatMessageSchema` 已区分 user/agent/system、primary kind 和 `agent_log:*` | 不需要 API/schema 迁移 |
| Store/API | `web/src/store.ts`、`web/src/api.ts` | POST messages 成功后把响应 messages 写入 `WorkbenchState.chat.messages` | 数据已到达前端，保持不变 |
| 前端测试 | `web/src/app.test.tsx` | sendMessage 测试只断言 store 最后一条消息；静态 render 测试覆盖 tool log/edit/run/proposal | 需增加 render-after-state-update 断言，锁定用户可见结果 |
| 样式 | `web/src/styles.css` | `.msg-text` 已有 `overflow-wrap:anywhere`，user/assistant/system 样式已存在 | 预计无需新增样式；如 DOM 组合暴露小间距问题，只做局部调整 |

## 设计方案

### 1. 接通唯一消息时间线

在 `chat-pane.tsx` 增加内部 `MessageTimeline` 组件，输入保持为现有 `ChatMessage[]`。组件只调用一次 `chatEntries(messages)`，按 entry 类型分派：

- `type=message` → `MessageRow`
- `type=assistantTurn` → `AssistantTurn`

`chatEntries` 继续作为唯一归组入口，不新增第二套 message-kind switch。React key 使用已有 message id 或 assistant turn id。

### 2. 收窄 EditSessionWorkspace 职责

`EditSessionWorkspace` 不再接收 `messages`，不再调用 `latestUserMessage` 或筛选 tool log。它只负责：

- dirty edit summary / idle card
- selected node context
- commit/discard actions
- 固定的 edit-session 辅助说明

删除因此失去调用点的 `latestUserMessage`、`isToolLogEntry`。这样 user message 和 tool log 只在 `MessageTimeline` 中出现一次。

### 3. ChatPane 布局顺序

滚动容器使用以下稳定顺序：

1. `EditSessionWorkspace`
2. `MessageTimeline`
3. `RunErrorCard`
4. 当前 legacy/pending `ProposalMessage`

Edit-session 是当前画布状态摘要；消息时间线保持自身顺序；run/proposal card 是当前可操作补充。`useEffect([messages])` 的自动滚动保持不变，使新回复到达后滚动到底部。

### 4. 安全与空数据

- 继续使用 React 文本节点渲染 `message.text`，不引入 `dangerouslySetInnerHTML`、HTML parser 或 Markdown 执行。
- `message.text === ''` 时渲染空文本，不填充推测性文案。
- `messages=[]` 时 `MessageTimeline` 返回 null；既有 edit-session idle state 仍存在，但不伪造聊天消息。
- 复用 `.msg-text` 的 `overflow-wrap:anywhere`；仅在 fresh render test 或人工检查证明需要时修改 CSS。

### 5. 测试策略

优先补在现有 `web/src/app.test.tsx`，不创建新的大型测试框架：

1. 构造包含 user、agent chat、system error 的 state，渲染 App/ChatPane，断言三条正文和顺序。
2. 扩展“posts composer messages”测试：store 更新后重新 `renderToStaticMarkup(<App />)`，断言“我是 Helixflow agent。”出现在 markup。
3. 构造 Agent reply + 多条 `agent_log:*`，断言普通 reply 和 tool group 都出现，日志文本不在普通 MessageRow 重复。
4. 保留既有 proposal、RunErrorCard、edit-session、IME 测试。

本 issue 不引入 Playwright 依赖。仓库后续仍需要浏览器 E2E，但当前回归可由组件 render test 确定性锁定。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1、P2、P3 | `MessageTimeline`、`MessageRow`、`AssistantTurn` | render state 含 user/agent/system，断言三类正文 |
| P4 | `chatEntries` 单次遍历与 entry map | 交错消息 fixture 断言正文索引顺序；日志不重复 |
| P5 | `chatEntries`、`AssistantTurn`、`ToolCallGroup` | Agent reply + logs 同时可见，group event count 正确 |
| P6 | ChatPane 滚动容器组合 | 既有 edit/proposal/run card 静态 render tests 全部保留 |
| P7 | `MessageTimeline` 空数组分支 | 空消息 fixture 不出现伪造 reply |
| P8 | 既有 `busy` props/composer | 既有 busy/disabled tests；本变更不修改 submit |
| P9 | `shouldSubmitComposerKey` | 既有 IME/Enter/Shift+Enter tests |
| P10 | 既有 `.msg-text` 样式 | 长文本 render + `npm run build`；人工检查聊天栏 |
| P11 | React text rendering | 植入 `<script>` 字符串，markup 中必须被转义且无 raw script node |
| P12 | `web/src/app.test.tsx` | sendMessage 后重渲染并断言 Agent reply markup |

## 数据流

1. 用户通过 composer 触发既有 `onSend(text)`。
2. Store 调用 `POST /api/workspaces/{workspace_id}/messages`。
3. 后端返回并持久化 messages；Store 更新 `WorkbenchState.chat.messages`。
4. App 把 `messages` 作为 props 传给 ChatPane。
5. `MessageTimeline` 调用 `chatEntries(messages)`：
   - user/system 直接成为 message entry；
   - `agent_log:*` 暂存并归入相邻 Agent turn；
   - Agent primary message 与日志组成 assistant turn。
6. renderer 输出 React 文本 DOM；不新增持久化、外部调用或 API 请求。

## 备选方案

- 在 ChatPane 中直接 `messages.map(MessageRow)`：拒绝。会丢失现有 Agent log 归组，并使 agent reply 使用错误的 user renderer。
- 继续只在 `EditSessionWorkspace` 中补普通回复：拒绝。会让 edit-session 继续承担聊天时间线职责，并只展示截断子集。
- 新建第二套 message normalizer：拒绝。现有 `chatEntries` 已表达所需归组规则，平行实现容易漂移。
- 本 issue 同时引入 Playwright：暂不采用。修复面应保持小；浏览器 E2E 缺口保留为独立测试基础设施任务。

## 风险

- Security：消息正文来自用户/Agent；必须保持 React 文本转义，不引入 HTML 注入面。
- Compatibility：完整渲染历史 messages 会比当前 tool-log-only UI 增加 DOM 数量；当前无分页/虚拟化。本 issue 保持现有完整 state 语义，后续如有大历史性能问题应单独设计窗口化。
- Performance：`chatEntries` 为 O(n)；每次 messages 更新执行一次。避免在 EditSessionWorkspace 再执行第二次。
- UX：完整历史可能改变左栏密度；保留现有折叠 tool log，并复用已有样式控制。
- Maintenance：必须删除重复 helper/路径，确保未来只有一个 message entry renderer。

## 测试计划

- [ ] Unit/component tests：user、agent chat、system/error 正文渲染与稳定顺序。
- [ ] Unit/component tests：Agent reply 与 `agent_log:*` 分组，正文不重复。
- [ ] Unit/component tests：空消息与 HTML-like 文本安全转义。
- [ ] Integration-style frontend test：`sendMessage` 更新 store 后重新渲染 App，回复正文可见。
- [ ] Regression tests：proposal、RunErrorCard、edit-session、IME、busy composer 不回退。
- [ ] Deterministic verification：`cd web && npm test`。
- [ ] Type/build verification：`cd web && npm run build`。
- [ ] SpecRail verification：`python3 checks/check_workflow.py --repo . --spec-dir specs/GH114`。
- [ ] Manual verification：启动本地前后端，发送普通中文消息，确认 user/Agent/error 可见；该项在有可用浏览器运行时时执行。

## 回滚方案

若新时间线造成不可接受的布局或性能回归，只回滚 `MessageTimeline` 挂载、EditSessionWorkspace 职责收窄及对应测试；后端 API、store schema 和持久化没有变化，无需数据迁移。回滚不得删除 GH-114 的失败测试证据，应先记录具体回归并重新进入 spec review。
