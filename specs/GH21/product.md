# Agent Turn Routing, PromptStack, and Tool Log Contract

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/21
Locale: zh-CN

## 背景

`docs/PROMPT_DESIGN.md` 已经定义 Helixflow 应采用分层 prompt stack，而不是继续把所有 agent 行为塞进少量硬编码 prompt 分支。`docs/AGENT_RUNTIME_PROVIDER_SPEC.md` 同时要求 chat、workflow proposal、run request、debug、artifact 等 turn mode 有明确边界。

当前本地实现已经有 `AgentSkill::Chat`、`AgentSkill::CreateWorkflow`、`AgentSkill::ModifyWorkflow`、runtime log、chat pane 分组等基础，但仍存在几个产品风险：

- 普通聊天、创建 workflow、修改 workflow、运行请求仍主要靠关键词分类和 `AgentSkill` 分支决定。
- `运行` 这类请求容易被当成 workflow change，而不是进入 backend-managed run request。
- tool/runtime 事件如果不被明确归类，容易污染主聊天消息。
- prompt 构造不可检查，调试 prompt bug 只能从最终字符串和 transcript 反推。

## 目标

1. 用户每一轮仍然走真实 agent runtime，但不同意图进入明确的 `TurnMode`。
2. 普通聊天返回 `out/reply.json`，不读取 ctx、不跑 shell、不生成 proposal。
3. 创建或修改 workflow 只生成 pending proposal，等待用户 apply/dismiss。
4. 明确运行请求不重写 workflow，而是进入 run request 或 run confirmation 路径。
5. tool/runtime logs 作为 assistant message 下的折叠证据显示，不作为主聊天正文。
6. 每个 turn 的 prompt sections 和 output contract 可检查。

## 非目标

- 不在本 spec 中实现多 agent runtime registry。
- 不在本 spec 中扩展 BYOK/provider 配置 UI。
- 不在本 spec 中推进 DesignArtifact 作为主产品路径。
- 不复制 OpenDesign 的完整 prompt 内容、plugin marketplace、MCP 注入或媒体生成体系。
- 不让 agent 直接调用 provider API、读取 provider secret、或直接写数据库。

## 用户场景

### 场景 1：普通聊天

用户输入“你好”或“你是谁”。系统仍启动 agent，但以 `Chat` mode 发送 prompt。agent 只能写 `out/reply.json`。UI 显示一条普通 assistant reply，不显示 proposal card。

### 场景 2：创建 workflow

用户在空 workspace 输入“帮我做一个文生图 workflow”。系统进入 `CreateWorkflow` mode。agent 读取 graph 和 catalog，写 `out/proposal.json`。UI 显示 pending proposal 和 graph preview。

### 场景 3：修改 workflow

用户已有 graph 后输入“把分辨率改成 1024”。系统进入 `ModifyWorkflow` mode。agent 只能输出最小 proposal diff。应用前不改变当前 graph version。

### 场景 4：运行当前 graph

用户输入“运行当前 workflow”。系统进入 `RunRequest` mode 或等价后端 run confirmation 路径。系统不能把该请求解释成“重新设计 graph”。

### 场景 5：运行失败后 debug

最近 run 失败后，用户输入“为什么失败了，帮我修复”。系统进入 `DebugWorkflow` mode，agent 读取 graph 和 run/error context，输出解释或修复 proposal。

## 产品需求

| ID | Requirement |
| --- | --- |
| PRD-01 | 每个 user turn 必须记录 `TurnMode`。 |
| PRD-02 | `Chat` mode 只能产生 reply，不得产生 proposal 或 run request。 |
| PRD-03 | `CreateWorkflow` 和 `ModifyWorkflow` mode 的结果必须经过 proposal validation。 |
| PRD-04 | `RunRequest` mode 不得修改 graph；外部 provider 调用仍由 backend 执行。 |
| PRD-05 | tool/runtime logs 必须挂在对应 assistant turn 下，默认折叠。 |
| PRD-06 | UI 不显示内部 lifecycle noise，例如 `thread.started`、`turn.completed` 作为主消息。 |
| PRD-07 | prompt stack 至少暴露 section keys、mode、output contract 以便调试。 |
| PRD-08 | mode classification 失败时必须明确报错或请求澄清，不能 silent fallback 到 graph modification。 |

## 验收标准

- 输入“你好”后，workspace messages 中只有 user message、assistant reply 和可选折叠 runtime log；没有 pending proposal。
- 输入“创建一个文生图 workflow”后，出现 pending proposal，graph 在 apply 前不变。
- 输入“把分辨率改成 1024”后，proposal 只包含最小 graph diff。
- 输入“运行当前 workflow”后，不产生 proposal redesign；若 graph 不可运行，返回明确错误或确认状态。
- runtime/tool log 在 UI 中可展开查看，但 assistant bubble 文本不包含原始 JSON event。
- prompt telemetry 能显示当前 turn 的 `TurnMode`、`PromptSectionKey` 列表和 `OutputContract`。

## 开放问题

1. `RunRequest` 第一版是否由 agent 产出 `out/run_request.json`，还是由 server classifier 直接创建 confirmation？
2. prompt section 内容是否全部持久化，还是只持久化 key、hash 和 debug-safe preview？
3. `DebugWorkflow` 在没有最近失败 run 时应直接回复，还是请求用户选择 run？
