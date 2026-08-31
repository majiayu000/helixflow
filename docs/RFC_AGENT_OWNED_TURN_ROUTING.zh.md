# RFC：由 Agent 决定聊天与画布动作

状态：已实现并完成本机真实验收

日期：2026-08-31

对应 Issue：<https://github.com/majiayu000/helixflow/issues/193>

## 1. 问题

Helixflow 当前已经具备真实的 Codex Agent、持久会话、结构化 `IntentPlan`、确定性图编译、版本事务、运行前检查、费用确认和画布状态同步。Agent 产出的不是一套写死的工作流模板，而是经过 schema 校验的高层意图。

但普通聊天入口仍有一处与这个架构不一致：`crates/agent/src/turn_mode.rs` 通过中英文关键词数组和字符串包含关系，在 Agent 启动前决定本轮属于 Chat、Create Workflow、Modify Workflow、Debug Workflow 还是 Run Request。无法匹配时固定回退到 Chat。

这会造成三类实际错误：

1. 用户使用关键词表之外的自然表达时，本应改画布的请求会被当成聊天。
2. “继续”“照刚才那个做”“把它换掉”等依赖会话上下文的表达无法可靠路由。
3. 增加更多关键词只会把自然语言理解继续固化在代码中，无法成为真正的 Agent 行为。

本 RFC 只解决这个入口问题，不重做已经存在的画布、编译器或执行引擎。

## 2. 调研结论

### 2.1 外部实践

tldraw Agent Starter Kit 将用户消息、选择、视口、结构化画布数据、截图、最近操作和会话历史作为 Agent 的“眼睛”，将经过 schema 验证的 action 作为“手”。模式决定 Agent 能看到的上下文和能执行的动作，而不是使用关键词决定用户意图。官方资料：

- <https://tldraw.dev/starter-kits/agent>
- <https://github.com/tldraw/tldraw/blob/main/templates/agent/README.md>

Vercel AI SDK 的官方资料同样把工具选择交给模型，再由宿主程序按 JSON Schema 验证参数和执行工具。这证明了合理边界是“模型判断语义，代码验证和执行”，而不是“代码猜语义，模型只填结果”。官方资料：

- <https://vercel.com/docs/ai-sdk>
- <https://vercel.com/kb/guide/how-to-build-ai-agents-with-vercel-and-the-ai-sdk>

仓库已有调研 `docs/references/AGENT_DRIVEN_CANVAS_WORKFLOW_RESEARCH.zh.md` 已得出一致结论：Agent 应输出高层、类型化动作，后端负责能力解析、图操作、校验、布局、版本和运行安全门。

### 2.2 Helixflow 现状

已经完成且继续保留的能力：

- Codex app-server 是生产 Agent runtime，不是测试 stub。
- graph edit 默认使用高层 `IntentPlan`，Agent 不负责节点 ID、坐标、binding ID 和底层连线细节。
- `canvas.get_state`、`canvas.submit_intent`、`canvas.request_run` 是受约束的动态工具。
- 后端编译、catalog resolution、graph validation、版本事务、回滚和运行确认仍是唯一执行边界。
- 显式 UI 动作可以直接声明 turn mode，例如中心空状态的“开始创建”。这是用户明确选择，不属于猜测或 hardcode。

唯一需要移除的语义判断是：无显式 mode 的自由文本不再经过关键词表。

## 3. 设计原则

1. **Agent 负责语义判断。** 自由文本的 turn mode 由真实 Codex 模型结合当前消息、有限会话历史和画布摘要决定。
2. **后端负责控制面。** mode 仍是封闭枚举；输出必须通过 schema 和枚举反序列化；图和运行仍由现有后端路径处理。
3. **没有静默降级。** 路由 runtime 失败、超时、缺文件、非法 JSON 或非法 mode 时，本轮明确失败；不得回退到 Chat 或关键词分类。
4. **显式意图优先。** UI 明确传入的合法 mode 直接采用，不额外消耗一次模型调用。
5. **路由不产生副作用。** 路由 Agent 无画布写工具、无 provider 工具、无网络，只能写一个 `route.json`。
6. **不让路由污染会话。** 语义路由使用临时 Codex thread；真正执行 Chat/Intent/Run 的 Agent turn 才恢复并持久化产品会话 thread。

## 4. 目标流程

```text
自由文本 + 有限历史 + 画布摘要
              ↓
     Codex semantic routing turn
              ↓
 out/route.json（语义复述 + 严格枚举）
              ↓
  Chat / Create / Modify / Debug / Run
              ↓
       现有 mode-specific Agent turn
              ↓
  Intent 编译 / 回复 / 运行确认等既有路径
```

显式 UI mode 的流程不经过第一步：

```text
用户点击明确动作 → typed turnMode → 现有 Agent turn
```

## 5. 路由契约

### 5.1 输出

路由 Agent 只能写：

```json
{
  "mode": "modify_workflow",
  "requestedAction": "保持现有链路，只调整第二阶段的参数。"
}
```

`mode` 只能是：

- `chat`
- `create_workflow`
- `modify_workflow`
- `debug_workflow`
- `run_request`

`requestedAction` 是模型对“当前用户要求 Helixflow 做什么”的单句复述，用来迫使路由先完成语义理解并留下可审计依据；它不参与后端图操作或运行决策。空值、超长值、额外字段和内部 `route` mode 都会被拒绝。动态工具 schema 在每个 mode 的参数定义处声明语义边界，模型仍负责选择，后端不再用关键词二次判断。

### 5.2 输入上下文

路由 turn 使用：

- 当前用户消息。
- 当前会话最近的有限历史，沿用现有 20 条、单条 1200 字符上限。
- workspace 和 base version 标识。
- 当前 graph 节点数、是否为空。
- 当前 selection 数量。

路由不需要读取完整 graph、catalog、provider 凭据或本机文件。真正进入 graph mode 后，现有 Agent 再读取权威结构化上下文。

### 5.3 语义定义

- Chat：回答问题、解释能力、澄清真实歧义，或没有可执行画布/运行意图。
- Create Workflow：创建新的工作流目标或在空画布上构建管线。
- Modify Workflow：修改、扩展、删除、重排当前工作流。
- Debug Workflow：依据失败运行或错误诊断并修复工作流。
- Run Request：请求执行当前工作流，不同时修改结构。

这些是后端已有的能力边界，不是自然语言关键词表。模型根据整段语义和历史选择其中一个，代码只验证选择是否合法。

## 6. 实现范围

### 6.1 Agent crate

- 新增内部路由 mode、`route.json` output contract 和严格反序列化。
- 新增 `AgentService::route_turn`，复用现有 runtime、timeout、事件和输出读取机制。
- 路由 prompt 明确五种语义，只暴露无副作用的 `agent.select_turn_mode` 动态工具。
- Chat 获得只读 compact canvas，并通过 `canvas.get_state` 查看当前图、通过 `canvas.submit_reply` 可靠提交回复；没有提案和运行工具。
- Intent 动态工具从当前 node catalog 生成合法 capability 枚举，避免把 node type 当成 capability；用户提供的字面输入仍由编译器确定性物化为输入节点。
- 删除关键词数组、substring classifier 和 ambiguous Chat fallback。
- `TurnModeSource` 改为 `Model` 或 `Explicit`。

内部路由 mode 不允许从 HTTP request 反序列化，避免客户端伪造控制面状态；API 只接受五种用户可见 mode。

### 6.2 Server crate

- 先校验 graph、解析 conversation 并读取历史。
- 会话历史只保留用户消息和可见 Agent 回复，`agent_log:*` 遥测不会挤掉真正上下文。
- 请求没有显式 mode 时，通过 `WorkbenchAgent::route_turn` 获取模型路由结果。
- 路由成功后再进入现有 durable turn、observation、Intent/Chat/Run 分支。
- 路由失败返回明确 API 错误，不创建错误的 Chat turn。
- 用户消息 metadata 记录 `turnModeSource: model`，便于验收和后续观测。

### 6.3 Web

正常 composer 继续不传 `turnMode`，由 Agent 决定。显式按钮继续传 typed mode。现有错误气泡和终止态处理继续使用；本期不增加模式选择器或新的配置面。

## 7. 为什么不采用其他方案

### 7.1 不扩充关键词

关键词无法覆盖上下文指代、否定、混合语言和新表达，且每次补词都会继续扩大 hardcode。

### 7.2 不让模型直接写完整 graph

这会破坏已经完成的 Intent compiler、catalog resolution、typed ports、版本事务和运行安全门。Agent 自由判断语义，不等于让 Agent 绕过确定性后端。

### 7.3 本期不合并成一个万能 action turn

单次模型调用同时选择并执行 action 最终可能降低延迟，但会要求重写 durable turn 创建、contract observation、mode-specific prompt、重试和 thread persistence。当前两阶段方案只替换错误的关键词入口，并复用已经验证过的执行路径。后续若观测证明双调用延迟不可接受，可独立设计统一 action envelope。

## 8. 验收标准

### 自动测试

1. 模型返回五种合法 route 和非空 `requestedAction` 时均能严格解析。
2. 缺文件、非法 JSON、额外字段、非法 mode、runtime failure 和 timeout 均明确失败。
3. 没有任何关键词或 graph-empty fallback 路径残留。
4. 无显式 mode 的 server 请求使用 Agent 路由结果，而不是消息文本。
5. 显式 UI mode 绕过模型路由。
6. metadata 区分 `model` 与 `explicit`。
7. 既有 Chat、Intent compile/apply、Debug 和 Run Request 回归通过。

### 真实验收

在隔离数据目录和端口启动本机 Helixflow，用真实 Codex runtime 依次验证：

1. 不含旧关键词的中文创建表达能生成并应用画布工作流。
2. 对已有图使用代词和上下文的修改表达能修改而不是只回复文字。
3. 产品解释问题只聊天，不改图。
4. 运行请求进入后端费用/确认路径；价格未知时允许显示 Unknown，不作为失败。
5. 无法路由或 runtime 故障时显示明确错误，不静默当成 Chat。

UI 验收同时检查聊天消息、画布节点/连线、版本变化、Agent 状态、重连后状态和浏览器控制台错误。

## 9. 非目标

- 不删除 GH130 legacy intent rollback；其删除仍受既有观察窗口约束。
- 不改变 provider/model 价格策略；Unknown 继续是合法展示。
- 不新增模型别名、关键词、规则引擎或自动模板。
- 不重写 React Flow 画布、版本存储、run engine 或 provider gateway。
- 不在 Agent OS 上部署或运行本次开发服务；研发和验收都在当前电脑完成。

## 10. 风险与观测

- **额外延迟和模型调用。** 只对没有显式 mode 的自由文本发生；记录 mode source 后可以量化。
- **模型误分类。** 后端类型、图校验和运行确认仍阻止越权副作用；真实语义用例进入回归集。
- **路由不可用。** 明确失败而不是静默降级，用户可重试或通过显式 UI 动作继续。
- **上下文注入。** 历史和用户消息继续按不可信 JSON 数据编码；路由 prompt 不提供工具和网络。

## 11. 完成定义

RFC、实现、单元/集成/Web/Playwright 测试、真实本机聊天画布验收全部通过；变更归属检查只包含本 RFC 范围；随后创建 issue 对应的单一 PR，并在 PR 中附上当次验证证据。

## 12. 2026-08-31 本机验收记录

验收只在当前 Mac 的隔离 worktree、临时数据目录和 `127.0.0.1:18787` 上进行，没有连接或改动 Agent OS，也没有影响原来的 `127.0.0.1:8787` 服务。运行 provider 使用 mock，Agent 使用真实 Codex app-server。

1. 自由中文创建请求由模型路由为 `create_workflow`，生成并自动应用 3 个节点、2 条类型化边的新版本：`input.text → llm.prompt_writer → image.generate`。
2. 上下文修改请求由模型路由为 `modify_workflow`，图片比例从 `16:9` 改为 `9:16`，其余拓扑保持不变，并产生新的可回滚版本。
3. 执行请求由模型路由为 `run_request`，通过 0.00 USD 自动费用门禁；mock run 的 3 个步骤全部成功并产出 text、text、image artifacts，运行没有改变画布版本。
4. Playwright 从真实页面新建对话并发送“用一句话概括当前工作流的输入、处理和输出。”；请求体没有 `turnMode`，模型路由为 `chat`，Agent 通过只读画布状态准确回答，3 个节点保持不变，浏览器控制台没有错误。
5. 实测中发现并修复了四个仅靠单测不易暴露的问题：路由缺少可靠提交工具、遥测日志挤占会话历史、Intent capability schema 未绑定实时 catalog、Chat 无法读取当前画布或可靠提交回复。所有失败均显式终止，没有静默回退到 Chat。
