# Technical Spec: Workbench Workflow JSON Export

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/26
Locale: zh-CN

## 输入资料

- `crates/server/src/main.rs`
- `crates/server/src/graph_files.rs`
- `crates/server/src/workspace_state.rs`
- `crates/server/src/proposal_routes.rs`
- `crates/store/src/lib.rs`
- `web/src/api.ts`
- `web/src/store.ts`
- `web/src/app.tsx`
- `web/src/components/top-bar.tsx`
- `web/src/types.ts`

## 当前实现摘要

- Store 已有 `Store::version(version_id)`，version record 包含 server-owned `graph_path`。
- `graph_files::read_graph_file` 已通过 safe relative path 读取 stored graph。
- `workspace_state` 返回 current version 对应的 `workflowGraph`，并可同时返回 pending proposal preview。
- TopBar 里已有 disabled Export icon，但没有真实行为。
- 前端 `WorkflowGraphSchema` 可验证导出响应 shape。

## 设计决策

1. Export 是 version action，不是 workspace canvas action。
2. 第一版 endpoint 使用 `GET /api/versions/{version_id}/export`，由前端传当前 `workspace.versionId`。
3. 后端只读取 stored version graph，不读取 proposal preview。
4. 后端在响应前做 export safety check；发现敏感字段或本地绝对路径时拒绝导出。
5. 前端 API client 解析 `WorkflowGraphSchema` 后才触发浏览器下载。

## API Contract

```http
GET /api/versions/{version_id}/export
```

Success response:

```json
{
  "schema_version": 1,
  "nodes": {},
  "edges": []
}
```

Errors:

- `404` when the version does not exist.
- `400` when the stored graph contains non-exportable content such as secret fields, auth headers, runtime metadata, or local absolute paths.
- `500` only for unexpected storage or JSON failures.

## Backend Design

### Route

Add `crates/server/src/version_routes.rs` with:

- `export_workflow_version(Path(version_id), State(state))`
- `ensure_exportable_graph(graph)`

Route steps:

1. Load the version record by `version_id`.
2. Read `version.graph_path` using `read_graph_file`.
3. Serialize/inspect graph JSON for unsafe export content.
4. Return `Json<WorkflowGraph>`.

### Safety Check

The export safety check scans the graph JSON recursively:

- Reject key names that indicate secrets, tokens, credentials, auth headers, cookies, or provider request headers.
- Reject known local absolute path string prefixes such as `/Users/`, `/home/`, `/tmp/`, `/private/`, `/Volumes/`, `~/`, and Windows drive paths.
- Reject runtime metadata keys such as `run_id`, `session_id`, `trace_id`, and `provider_request_id`.
- Error messages must identify the JSON field path but must not echo the unsafe value.

The first version rejects unsafe export rather than redacting. This avoids silently returning a workflow that differs from the stored version.

## Frontend Design

### API Client

Add `exportWorkflowVersion(versionId: string): Promise<WorkflowGraph>` in `web/src/api.ts`.

- Request `GET /api/versions/{versionId}/export`.
- Parse success with `WorkflowGraphSchema`.
- Surface backend `error` message on failure.

### Store

Add `exportWorkflow(): Promise<WorkflowGraph | null>` to `web/src/store.ts`.

- Uses current `state.workspace.versionId`.
- Does not read `pendingProposal.previewGraph`.
- On failure, appends a system error using the existing visible error path.

### UI

Wire TopBar Export button:

- Enabled when current workspace has a non-empty version id and the app is not busy.
- Calls store export action.
- Downloads a `.json` file from the parsed backend graph.
- Does not refresh or mutate workspace state.

## Product Requirement Mapping

| Requirement | Implementation Area | Verification |
| --- | --- | --- |
| PRD-01, PRD-02 | `top-bar.tsx`, `app.tsx`, `api.ts`, `store.ts` | Web test asserts version export API is called |
| PRD-03, PRD-04 | `version_routes.rs`, store action | Server and web tests with pending proposal |
| PRD-05 | `WorkflowGraphSchema` parse | Web test with parsed graph |
| PRD-06 | `version_routes.rs` safety check | Server test rejects unsafe params |
| PRD-07 | Store error path | Existing error append pattern plus export action |

## Risks

- Rejecting unsafe graphs may surprise users if future nodes intentionally store local files; that should be handled with explicit artifact import/export policy in a later issue.
- Browser download APIs are unavailable during SSR; the click handler must only run in the browser.
- The endpoint exports by version id, so future authorization must ensure the active user can access that version.

## Verification Commands

```sh
cargo fmt --check
cargo check --workspace
cargo test -p helixflow-server version_routes
cargo test --workspace
cd web && npm test -- app.test.tsx
cd web && npm run build
git diff --check
```

## Rollback Plan

- Remove `GET /api/versions/{version_id}/export` and `version_routes.rs`.
- Revert TopBar Export to disabled.
- Remove the API/store export action and associated tests.
