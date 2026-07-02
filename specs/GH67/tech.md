# Tech Spec

## Linked Issue

GH-67

## Product Spec

`specs/GH67/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Agent service | `crates/agent/src/service.rs` | 单次 `codex exec`,读取 `out/proposal.json` | 重试循环主入口 |
| Prompt stack | `crates/agent/src/prompt_stack.rs` | 分层生成 agent 指令/context | 需要加入上一轮 validation error |
| Proposal validation | `GraphService::preview_proposal`, server proposal flow | 产出结构化校验错误 | 回喂内容来源 |
| Runtime logs/status | run/message event 通道 | 展示 agent/run 进度 | 每轮进度可见 |

## 设计方案

### 1. Retry loop

把 agent turn 从单次 `run_codex_once` 提升为 loop: build context -> invoke Codex CLI -> parse proposal -> validate -> success break / failure append feedback -> next attempt。默认 `max_attempts=3`,可配置。

### 2. Feedback layer

prompt stack 增加 `previous_validation_errors` layer,包含 machine-readable summary: attempt number、error kind、opIndex/field、message、compact graph facts。避免把完整大 graph 或 secret 写入 prompt。

### 3. Validation path

每轮都走同一个 proposal parsing + `preview_proposal`/graph validation。格式错误也转为 feedback。只有 validation pass 才创建 pending proposal / proposal_pending message。

### 4. Runtime status

每轮开始、validation failed、retrying、validated、exhausted 都写 runtime log/status。超限返回最后错误和 attempt count。

### 5. Cancellation

复用 run interrupt/cancel token;取消时 kill 当前 Codex CLI 子进程或停止等待,不再启动下一轮。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 每轮同一校验 | service loop | fake codex outputs invalid then valid |
| P2 结构化回喂 | prompt_stack layer | rendered prompt contains error summary |
| P3 max attempts | config/service | always-invalid test stops at 3 |
| P4 success stops | proposal persistence | valid second attempt creates one pending proposal |
| P5 runtime logs | status/log writer | log assertions per attempt |
| P6 cancel | process/token handling | cancel during attempt test |

## 数据流

workspace context -> Codex CLI attempt -> proposal file parse -> preview/validate -> feedback layer or pending proposal -> runtime log/status events.

## 备选方案

- 直接 API tool-calling:owner 已明确暂不采用,放弃。
- 用户手动复制错误再重试:不能自动自愈,放弃。

## 风险

- Security: feedback 不得包含 secrets/API keys/artifact bytes。
- Compatibility: Codex CLI failure modes需保持现有错误可诊断。
- Performance: 多轮会增加延迟,默认上限必须小。
- Maintenance: loop 状态机需测试覆盖。

## 测试计划

- [ ] Unit tests: prompt feedback layer、max attempts。
- [ ] Integration tests: fake Codex CLI invalid->valid、always invalid、invalid JSON、cancel。
- [ ] Manual verification: 构造坏图自愈场景。

## 回滚方案

将 `max_attempts` 配置为 1 即恢复单轮行为。

