# Technical Spec: Failed Run Diagnosis Cards And Minimal Fix Proposals

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/27
Locale: zh-CN

## 输入资料

- `crates/store/src/run_records.rs`
- `crates/run/src/lib.rs`
- `crates/server/src/workspace_state.rs`
- `crates/server/src/workbench_message.rs`
- `crates/server/src/workbench_payload.rs`
- `crates/agent/src/turn_mode.rs`
- `crates/agent/src/prompt_stack.rs`
- `web/src/types.ts`
- `web/src/store.ts`
- `web/src/app.tsx`
- `web/src/components/chat-pane.tsx`
- `web/src/components/graph-canvas.tsx`
- `web/src/app.test.tsx`
- `web/src/styles.css`

## 当前实现摘要

- Run execution 已在 step/run 失败时写入 `error_json`。
- Workspace state 当前只暴露 run status、steps 和 cost，没有 structured error payload。
- GraphCanvas 已根据 run step state 对 failed node 加 `node--err` 和“失败”标记。
- `classify_turn_mode` 已能把“报错/修复/failed/error”等关键词路由到 `DebugWorkflow`。
- `workbench_message.rs` 已为 `DebugWorkflow` 构造 latest run context，并有测试覆盖。
- ChatPane 目前只显示普通 message、proposal card 和 agent logs，没有 failed-run ErrorCard。

## 设计决策

1. 在 workspace/run payload 中新增 `error` 字段：
   - `run.error: { summary, raw } | null`
   - `run.steps[].error: { summary, raw } | null`
2. Error summary 从 persisted `error_json` 中提取：
   - 优先 JSON object 的 `error` string。
   - 否则使用短 raw 字符串作为 summary。
   - `raw` 保留截断后的原始文本，供 ErrorCard 展开。
3. WebSocket event 也把 `data.error` 转成同样的前端 error shape。
4. ChatPane 增加 `run` prop，根据 latest failed run 渲染 ErrorCard。
5. ErrorCard 默认不渲染 raw error 文本，点击后显示。
6. DebugWorkflow 保持现有 proposal path：agent 生成 pending proposal，用户通过现有 apply/dismiss path 审核。

## Data Contract

```json
{
  "run": {
    "status": "failed",
    "error": {
      "summary": "provider rejected duration",
      "raw": "{\"error\":\"provider rejected duration\"}"
    },
    "steps": [
      {
        "nodeId": "video",
        "state": "failed",
        "error": {
          "summary": "provider rejected duration",
          "raw": "{\"error\":\"provider rejected duration\"}"
        }
      }
    ]
  }
}
```

## Backend Design

### Workspace State

Enhance `run_payload` in `crates/server/src/workspace_state.rs`:

- Add `error` to run payload.
- Add `error` to each step payload.
- Add helper to parse `error_json` into summary/raw.
- Truncate summary/raw to bounded lengths.

### DebugWorkflow

Keep `debug_run_context` and `format_debug_run_context` as the source for agent context. Extend tests to prove:

- latest failed run context includes run/step error.
- DebugWorkflow creates a pending proposal.
- current workspace version remains unchanged until proposal apply.

## Frontend Design

### Types

Add a shared error schema in `web/src/types.ts` and attach it to `RunSchema` and `RunStepSchema`.

### Store

Update `applyRunEvent`:

- `node.state` with `data.error` updates matching step `error`.
- `run.failed` with `data.error` updates `run.error`.

### UI

Add `ErrorCard` inside `ChatPane`:

- Shows failed run label/status and failed node ids.
- Shows summary text.
- Has a toggle to reveal raw error.
- Does not render raw error by default.

No GraphCanvas change is required because failed node styling already exists and should remain covered by tests.

## Product Requirement Mapping

| Requirement | Implementation Area | Verification |
| --- | --- | --- |
| PRD-01, PRD-02 | `workspace_state.rs`, `types.ts` | server/web tests |
| PRD-03, PRD-04 | `chat-pane.tsx`, `styles.css` | static render tests |
| PRD-05 | existing `graph-canvas.tsx` | existing + static render tests |
| PRD-06, PRD-07 | `turn_mode.rs`, `workbench_message.rs` | server tests |
| PRD-08 | `workbench_message.rs`, proposal apply path | server tests |

## Risks

- Raw provider errors may contain noisy implementation detail. First version keeps raw collapsed and bounded.
- Multiple failed steps can produce long cards. First version summarizes failed node ids and uses the first available summary.
- Secret redaction should become a cross-cutting provider error policy if real providers can leak credentials.

## Verification Commands

```sh
cargo fmt --check
cargo check --workspace
cargo test -p helixflow-server workspace_state
cargo test -p helixflow-server workbench_message
cargo test --workspace
cd web && npm test -- app.test.tsx
cd web && npm run build
git diff --check
```

## Rollback Plan

- Remove `error` fields from run/step payloads.
- Remove ErrorCard rendering and CSS.
- Keep existing failed node styling and DebugWorkflow routing unchanged.
