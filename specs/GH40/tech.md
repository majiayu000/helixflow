# Technical Spec: Runtime Provider Catalog And State

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/40
Locale: zh-CN

## 输入资料

- `docs/ROADMAP.md`
- `docs/AGENT_RUNTIME_PROVIDER_SPEC.md`
- `docs/AGENT_RUNTIME_PROVIDER_VALIDATION.md`
- `crates/gateway/src/lib.rs`
- `crates/gateway/src/runtime_provider.rs`
- `crates/server/src/app_state.rs`
- `crates/server/src/workspace_state.rs`
- `crates/server/src/workbench_payload.rs`
- `crates/agent/src/lib.rs`
- `crates/agent/src/prompt_stack.rs`
- `crates/agent/src/tests.rs`
- `web/src/types.ts`
- `web/src/store.ts`
- `web/src/components/top-bar.tsx`
- `web/src/app.test.tsx`

## 当前实现摘要

- `helixflow-gateway` 已有 `Provider` trait、`MockProvider`、`RuntimeProvider::Mock` 和 `RuntimeProvider::Unavailable`。
- `crates/server/src/app_state.rs` 通过 `HELIXFLOW_RUNTIME_PROVIDER` 选择 provider；默认是 `mock`，未知 provider 会变成 unavailable。
- `persist_runtime_provider_status` 会把 provider health 写入 store，但 workspace state payload 没有把 provider catalog/status 明确返回给前端。
- `RunService<RuntimeProvider>` 已经通过 provider trait 执行 estimate/invoke/cancel。
- `crates/registry/src/lib.rs` 的 node definitions 仍以 `mock` provider/capability 为主。
- `AgentSession` 当前会写 `ctx/graph.json` 和 `ctx/node_defs/catalog.json`，但没有写 spec 中要求的 workflow backend、runtime provider、API connector catalog。
- `web/src/types.ts` 对 provider state 有 default `mock` shape，可能掩盖后端 state 缺失。
- `web/src/components/top-bar.tsx` 仍包含 Atlas 配置文案，但当前后端没有 Atlas connector。

## 设计决策

1. 第一版只建立 provider truth contract，不接真实外部 provider。
2. Provider catalog 由 server/backend 构造并放入 workspace state；frontend 不再通过 schema default 生成 provider truth。
3. Runtime provider state 至少包含当前 configured provider id、display label、status、enabled/unavailable、message、capability summaries。
4. `mock` provider 保留，但 display label 必须清楚表达它是本地测试 provider。
5. 未知 provider 仍使用 `RuntimeProvider::Unavailable`，并在 state 中显示 unavailable；run path 不做 mock fallback。
6. Agent ctx catalogs 使用不含 secrets 的静态/派生 JSON。第一版可用最小 catalog，后续真实 provider issue 再扩展 connector-specific fields。
7. Frontend TopBar 展示 provider status 时只读 state；缺失 provider state 应显示 backend state missing/error，而不是 mock default。

## Data Contract

Workspace state 增加 `providers` 字段：

```json
{
  "providers": {
    "defaultProvider": "mock",
    "runtimeProviders": [
      {
        "id": "mock",
        "label": "Mock Provider",
        "kind": "local_test",
        "enabled": true,
        "status": "healthy",
        "message": "mock provider ready",
        "capabilities": ["image_generate", "text_to_video", "prompt_writer"]
      }
    ],
    "workflowBackends": [
      {
        "id": "helixflow_graph",
        "label": "Helixflow Graph",
        "status": "healthy"
      }
    ],
    "apiConnectors": [
      {
        "id": "mock.text_to_video",
        "provider": "mock",
        "capability": "text_to_video",
        "status": "healthy"
      }
    ]
  }
}
```

Unknown env provider example:

```json
{
  "providers": {
    "defaultProvider": "openai",
    "runtimeProviders": [
      {
        "id": "openai",
        "label": "openai",
        "kind": "unavailable",
        "enabled": false,
        "status": "unavailable",
        "message": "runtime provider `openai` is not configured by this build",
        "capabilities": []
      }
    ],
    "workflowBackends": [],
    "apiConnectors": []
  }
}
```

Field naming follows the existing frontend API boundary camelCase. Rust structs may use snake_case with serde rename where needed.

## Backend Design

### Gateway/provider catalog

Add or expose a small serializable catalog type in `helixflow-gateway` or server-local payload code:

- `RuntimeProviderSummary`
- `WorkflowBackendSummary`
- `ApiConnectorSummary`
- `ProviderCatalogPayload`

`RuntimeProvider::Mock` should derive capability names from `MockProvider::catalog()` or equivalent provider metadata. `RuntimeProvider::Unavailable` should expose no capabilities and include its unavailable reason.

### App state/provider selection

Keep `default_runtime_provider()` semantics:

- missing env -> `RuntimeProvider::mock()`;
- `mock` -> `RuntimeProvider::mock()`;
- empty env -> unavailable invalid provider;
- any other id -> unavailable provider with explicit reason.

Do not add fallback behavior in run execution. Existing unavailable provider tests should remain.

### Workspace state

`workspace_state.rs` should include the provider catalog payload in the state response. Tests should cover:

- default mock provider payload;
- unknown env/provider payload;
- no provider payload defaulting in frontend is required.

### Agent session ctx

Graph-changing modes should write:

- `ctx/workflow_backends/catalog.json`
- `ctx/runtime_providers/catalog.json`
- `ctx/api_connectors/catalog.json`

These files should be generated from the same safe catalog model used by server state or from an equivalent agent-safe projection. Tests should assert:

- files exist for proposal modes;
- content includes provider/capability identifiers;
- content does not include `PROVIDER_API_KEY`, raw auth headers, signed URLs, or other secret values.

Chat mode should continue to avoid graph/provider ctx reads unless the current prompt contract explicitly needs them.

## Frontend Design

### Types

Update `web/src/types.ts`:

- remove provider truth defaults that fabricate `mock`;
- parse `providers` from backend state;
- keep optional/nullable handling only for backwards-compatible missing server payload, but render it as unavailable/missing state.

### Store

Keep state hydration source as backend `/state`. Do not derive provider truth from local constants.

### TopBar

Replace hardcoded Atlas status with state-driven provider label/status:

- healthy enabled provider: show provider label and healthy/real/mock-local marker;
- unavailable provider: show provider label and unavailable message;
- missing provider state: show a backend state missing marker.

Do not introduce a settings panel or provider selector in this issue.

## Product Requirement Mapping

| Requirement | Implementation Area | Verification |
| --- | --- | --- |
| PRD-01 | `workspace_state.rs`, `workbench_payload.rs`, `web/src/types.ts` | server state tests, web parse/render tests |
| PRD-02 | `RuntimeProvider::Mock`, provider catalog projection, run tests | gateway/server/run tests |
| PRD-03 | `default_runtime_provider`, unavailable provider state, run path | app_state tests, run/route tests |
| PRD-04 | `crates/agent/src/lib.rs`, agent ctx tests | `cargo test -p helixflow-agent` |
| PRD-05 | safe catalog projection, agent tests | secret redaction assertions |
| PRD-06 | `top-bar.tsx`, `app.test.tsx` | web tests and grep guard |
| PRD-07 | existing routes and tests | full workspace and web test suite |

## Risks

- If provider catalog is too generic, later real provider work may need shape changes. Keep first version minimal but explicit.
- If frontend treats missing provider state as mock for compatibility, the core product requirement fails. Missing state should be visible as missing/unavailable.
- Mock provider remains necessary for tests and local development; UI copy must avoid presenting it as real external execution.
- Agent ctx catalog generation must avoid leaking env vars or local filesystem paths.

## Verification Commands

```sh
cargo fmt --check
cargo check --workspace
cargo test -p helixflow-gateway
cargo test -p helixflow-server app_state
cargo test -p helixflow-server workspace_state
cargo test -p helixflow-agent
cargo test --workspace
cd web && npm test -- app.test.tsx
cd web && npm run build
git grep -n "Atlas 已配置\\|Atlas 未配置" -- web/src && exit 1 || true
git diff --check
```

Manual smoke after implementation:

```sh
HELIXFLOW_BIND_ADDR=127.0.0.1:8790 HELIXFLOW_DATA_DIR=/tmp/helixflow-gh40 cargo run -p helixflow-server
curl -s http://127.0.0.1:8790/api/workspaces
curl -s http://127.0.0.1:8790/api/workspaces/{workspace_id}/state
HELIXFLOW_RUNTIME_PROVIDER=openai HELIXFLOW_BIND_ADDR=127.0.0.1:8791 HELIXFLOW_DATA_DIR=/tmp/helixflow-gh40-unavailable cargo run -p helixflow-server
curl -s http://127.0.0.1:8791/api/workspaces/{workspace_id}/state
```

## Rollback Plan

- Remove `providers` from workspace state payload and frontend rendering.
- Remove agent ctx catalog files.
- Restore previous TopBar provider copy.
- Existing mock provider run path remains available throughout rollback.
