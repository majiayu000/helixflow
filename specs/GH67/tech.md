# Tech Spec

## Linked Issue

GH-67

## Product Spec

`specs/GH67/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Agent service | `crates/agent/src/service.rs` | `propose_graph_change` 调一次 `run_agent_turn`,再 `read_validated_proposal` | 循环入口在这里 |
| Runtime contract | `crates/agent/src/runtime.rs`, `codex_runtime` | runtime 支持 start/send/next_event,每 turn 写 out/proposal.json | 可选择同 session 多次 send 或逐轮新 session |
| Prompt stack | `crates/agent/src/prompt_stack.rs` | prompt stack 有 mode/context/output contract | 需要加入上一轮 validation error 层 |
| Validation | `crates/agent/src/output.rs`, `crates/graph/src/lib.rs` | `read_validated_proposal` 和 GraphService preview 会给结构化错误 | retry 输入的事实来源 |
| UI logs | `crates/run/src/EventBus`, `web/src/store.ts` | agent.status / agent.log 已可显示 | 每轮状态复用现有通道 |

## 设计方案

在 `AgentService::propose_graph_change` 中引入 `max_proposal_rounds` 配置,默认 3。循环每轮执行 runtime turn 并读取 proposal。读取或 preview 失败时,捕获错误为 `ValidationFeedback`,emit `agent.status` round_failed,然后构造下一轮 `AgentTurn`:同一 user intent + 当前 graph context + 上轮错误摘要 + 明确要求只修正 proposal。通过则 emit proposal ready 并返回。

优先复用同一个 `AgentSession` 和 runtime handle,连续 `send` 多个 turn,使 Codex CLI 可保留上下文;如果 runtime 不支持续接,实现可退化为每轮新 session,但事件中的 `session_id` 需要能把 rounds 聚合给 UI。每轮 out/proposal.json 读取前要清理或版本化上轮 output,避免读到旧文件。

retry prompt 必须短而结构化:包含 error kind、路径/字段、validator message、当前 graph/version id 和允许的 proposal ops。不得包含真实密钥、未声明文件或任意本地路径。Chat/ReplyJson 模式仍走单轮。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1、P4 | AgentService loop | fake runtime 首轮坏 proposal、次轮好 proposal 测试 |
| P2、P6 | prompt stack/retry turn | 单测:retry turn 含 validation error 且 Chat 不进入 loop |
| P3 | max rounds/exhausted | fake runtime 连续失败后返回最后错误 |
| P5 | EventBus logs | agent tests:round status/log 顺序可见 |

## 数据流

user request -> create session/context -> round 1 codex exec -> read/preview proposal -> fail -> emit log -> retry turn with validation feedback -> pass -> return ValidatedAgentProposal -> server creates pending proposal。

## 备选方案

- 直接切 OpenAI API tool-calling:被否,owner 决策继续用 Codex CLI。
- 把 invalid proposal 交给用户再让用户重试:被否,没有利用结构化 validator 自修复。

## 风险

- Security: retry feedback 不能泄露 ctx 外文件、secrets 或绝对路径。
- Compatibility: runtime 续接能力不一,需要清晰 fallback。
- Performance: 多轮 Codex exec 增加耗时,默认上限必须小。
- Maintenance: 测试要用 fake runtime,不依赖真实 Codex CLI。

## 测试计划

- [ ] Unit tests: fake runtime 坏->好、自愈成功;连续失败 exhausted;Chat 单轮。
- [ ] Integration tests: server agent run log 显示每轮状态,最终 pending proposal 只在合法 proposal 后出现。
- [ ] Manual verification: 构造首轮非法 op 的测试 agent,确认 ≤3 轮交付合法 proposal。

## 回滚方案

把 `max_proposal_rounds` 设为 1 或 revert loop,恢复单轮行为;无 schema 迁移。
