# Technical Spec: Agent Turn Routing, PromptStack, and Tool Log Contract

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/21
Locale: zh-CN

## 输入资料

- `docs/PROMPT_DESIGN.md`
- `docs/AGENT_RUNTIME_PROVIDER_SPEC.md`
- `docs/AGENT_RUNTIME_PROVIDER_VALIDATION.md`
- `SPEC_WORKFLOW_ORCHESTRATOR.md`
- 当前实现中的 `crates/agent`, `crates/server`, `web/src/components/chat-pane.tsx`

## 当前实现摘要

- `crates/server/src/chat_intent.rs` 目前只有 `ChatOnly` 与 `WorkflowChange` 两类。
- `crates/agent/src/lib.rs` 通过 `AgentSkill` 组装 prompt，尚未表达 `PromptStack`。
- `crates/agent/src/contract.rs` 已经能为 chat、design artifact、workflow proposal 写 ctx/out contract。
- `crates/server/src/workbench.rs` 已能写入 user/agent/system messages、proposal、run request。
- 前端已经有 chat log grouping，但事件过滤和 message kind contract 需要和后端固定下来。

## 设计决策

1. `TurnMode` 是当前 turn 的行为分类；`AgentSkill` 可以短期保留为兼容输出 contract 的桥。
2. `PromptStack` 是可测试的数据结构，最后才 render 成 Codex CLI prompt 字符串。
3. `OutputContract` 决定 backend 读取哪个 out 文件，以及如何校验。
4. `RunRequest` 第一版不直接调用 provider；它只请求 backend 创建 run confirmation 或返回不可运行错误。
5. tool/runtime logs 通过 typed message kind 进入 UI，不和 assistant reply 合并。

## 类型设计

```rust
pub enum TurnMode {
    Chat,
    CreateWorkflow,
    ModifyWorkflow,
    DebugWorkflow,
    RunRequest,
}

pub enum OutputContract {
    ReplyJson,
    ProposalJson,
    RunRequestJson,
}

pub enum PromptSectionKey {
    ModeOverride,
    DaemonSystem,
    RuntimeTool,
    RunContext,
    WorkflowBackend,
    RuntimeProvider,
    ApiConnectorCatalog,
    System,
    EchoGuard,
    UserRequest,
    AttachmentHint,
    CommentHint,
}

pub struct PromptSection {
    pub key: PromptSectionKey,
    pub title: String,
    pub body: String,
    pub capture_content: bool,
}

pub struct PromptStack {
    pub mode: TurnMode,
    pub sections: Vec<PromptSection>,
    pub output_contract: OutputContract,
}
```

## 后端拆分

### 1. Agent crate

Add or update:

- `crates/agent/src/prompt_stack.rs`
- `crates/agent/src/turn_mode.rs`
- `crates/agent/src/contract.rs`
- `crates/agent/src/tests.rs`

Responsibilities:

- Build prompt stack from `AgentSessionRequest`.
- Render prompt stack for Codex runtime.
- Persist debug-safe prompt metadata in transcript or sidecar JSON.
- Read and validate `out/reply.json`, `out/proposal.json`, and optional `out/run_request.json`.

### 2. Server crate

Add or update:

- `crates/server/src/chat_intent.rs`
- `crates/server/src/workbench.rs`
- `crates/server/src/workbench_view.rs`
- `crates/server/src/agent_transcript.rs`

Responsibilities:

- Classify user text plus workspace/run state into `TurnMode`.
- Route `Chat` to `answer_chat`.
- Route `CreateWorkflow` and `ModifyWorkflow` to proposal flow.
- Route `RunRequest` to backend run request/confirmation without graph redesign.
- Route `DebugWorkflow` using recent run/error context when available.
- Store typed messages for reply, proposal, error, and agent log.

### 3. Web app

Add or update:

- `web/src/types.ts`
- `web/src/store.ts`
- `web/src/components/chat-pane.tsx`
- `web/src/app.test.tsx`

Responsibilities:

- Render assistant reply separately from agent log groups.
- Collapse runtime/tool logs by default.
- Hide lifecycle noise from primary chat timeline.
- Preserve proposal cards and pending confirmation UI.
- Optionally expose prompt metadata in a debug-only affordance.

## Routing Table

| Condition | TurnMode | OutputContract | Expected backend action |
| --- | --- | --- | --- |
| greeting, identity, general explanation | `Chat` | `ReplyJson` | persist assistant reply |
| empty graph + workflow creation intent | `CreateWorkflow` | `ProposalJson` | create pending proposal |
| existing graph + modification intent | `ModifyWorkflow` | `ProposalJson` | create pending proposal |
| explicit run/queue/generate current graph | `RunRequest` | `RunRequestJson` or backend direct | create run confirmation or error |
| failed/latest run + fix/explain intent | `DebugWorkflow` | `ReplyJson` or `ProposalJson` | explain or create fix proposal |

## Data And API Contract

### Message kind values

Keep existing values when possible, but normalize the durable contract:

- `text`
- `chat`
- `proposal_pending`
- `proposal_applied`
- `proposal_dismissed`
- `run_requested`
- `run_failed`
- `agent_log:status`
- `agent_log:tool_call`
- `agent_log:tool_result`
- `agent_log:error`

Unknown `agent_log:*` values may render in a generic collapsed group. Unknown primary message kinds must not silently render as successful assistant replies.

### Prompt metadata

The first implementation can store:

```json
{
  "mode": "Chat",
  "output_contract": "ReplyJson",
  "sections": [
    { "key": "ModeOverride", "capture_content": true },
    { "key": "RuntimeTool", "capture_content": false },
    { "key": "UserRequest", "capture_content": true }
  ]
}
```

Do not store provider secrets, raw auth headers, signed URLs, or unrestricted local paths in prompt metadata.

## Implementation Steps

1. Add `TurnMode`, `OutputContract`, `PromptSectionKey`, `PromptSection`, and `PromptStack`.
2. Move existing prompt strings from `CodexRuntime::command_spec_for_turn` and `instructions_markdown` into prompt stack builders.
3. Replace binary `ChatIntent` with a routing function that returns `TurnMode`.
4. Add `RunRequest` routing so “运行当前 workflow” does not call graph proposal generation.
5. Normalize runtime log event kinds before persisting chat messages.
6. Update frontend schemas and chat pane grouping to treat logs as evidence under assistant turns.
7. Add regression tests for the routing table and UI grouping.

## Verification Commands

```sh
cargo test -p helixflow-agent
cargo test -p helixflow-server chat_intent
cargo test -p helixflow-server workbench
cd web && npm test -- app.test.tsx
cd web && npm run build
```

SpecRail repository validation, if the repo later adds the checker:

```sh
python3 checks/check_workflow.py --repo . --spec-dir specs/GH21
```

## Risks

- Keyword routing can still misclassify ambiguous Chinese prompts; tests must include Chinese and English examples.
- Capturing full prompt content can leak local paths or credentials if section boundaries are wrong.
- `RunRequest` can conflict with manual Queue semantics if both paths create runs differently.
- Large UI CSS files should not grow further while adding chat log states.

## Out Of Scope Follow-Ups

- Multi runtime registry and diagnostics.
- BYOK/provider connection test and model catalog.
- Dynamic node catalog from provider capabilities.
- DesignArtifact product lane.
- Full SpecRail scaffold for this repository.
