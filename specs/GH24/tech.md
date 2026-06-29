# Technical Spec: Workbench Version Undo And Restore

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/24
Locale: zh-CN

## 输入资料

- `crates/store/src/lib.rs`
- `crates/store/src/workspace_records.rs`
- `crates/server/src/main.rs`
- `crates/server/src/workspace_state.rs`
- `crates/server/src/proposal_routes.rs`
- `crates/server/src/graph_files.rs`
- `web/src/api.ts`
- `web/src/store.ts`
- `web/src/app.tsx`
- `web/src/components/top-bar.tsx`
- `web/src/components/run-panels.tsx`
- `web/src/types.ts`

## 当前实现摘要

- `versions` 表已有 `source IN ('manual', 'proposal', 'restore')` 和 `parent_id`。
- `Store::create_version_after` 会创建新 version 并原子更新 workspace current version。
- Proposal apply 已通过 `GraphService::apply_proposal` 检查 proposal base version 是否等于 current version。
- History payload 已包含 version/run/proposal 项，但 version 项没有 restore 操作按钮。
- TopBar Undo 目前是 disabled placeholder。

## 设计决策

1. Undo/restore 都创建新的 `restore` version，不删除、不改写旧 version。
2. Restore version 复用目标 version 的 immutable `graph_path` 和 `graph_hash`，避免复制 graph 文件造成重复存储。
3. 新 restore version 的 `parent_id` 指向执行操作前的 current version，用于审计和后续 Undo。
4. Undo target 优先使用 current version 的 `parent_id`；没有 parent 时使用 version index 的上一条记录。
5. Restore 指定已经 current 的 version 返回 conflict，避免制造无意义 duplicate record。
6. Stale proposal apply 映射为 `409 Conflict`，不是普通 `400 Bad Request`。

## API Contract

### Undo current version

```http
POST /api/workspaces/{workspace_id}/versions/undo
```

Success response: full `WorkbenchState` JSON.

Errors:

- `404` when workspace is missing.
- `409` when workspace has no current version or no undo target.

### Restore a version

```http
POST /api/workspaces/{workspace_id}/versions/{version_id}/restore
```

Success response: full `WorkbenchState` JSON.

Errors:

- `404` when version is missing or belongs to another workspace.
- `409` when restoring the current version.

## Backend Design

Add handlers in `crates/server/src/version_routes.rs`:

- `undo_workspace_version(Path(workspace_id), State(state))`
- `restore_workspace_version(Path((workspace_id, version_id)), State(state))`

Helper behavior:

1. Load workspace and versions for workspace.
2. Resolve current version from `workspace.cur_version_id`.
3. For undo, resolve target via current `parent_id` or previous version index.
4. For restore, load target version and verify workspace ownership.
5. Create `NewVersion { source: VersionSource::Restore, graph_path: target.graph_path, graph_hash: target.graph_hash, parent_id: Some(current.id) }` through `create_version_after`.
6. Return `workspace_state_value`.

Add proposal apply conflict mapping:

- When `GraphService::apply_proposal` returns `GraphError::ProposalSuperseded`, return `409 Conflict`.
- Other validation errors remain `400 Bad Request`.

## Frontend Design

### API Client

Add:

- `undoWorkspaceVersion(workspaceId: string): Promise<WorkbenchState>`
- `restoreWorkspaceVersion(workspaceId: string, versionId: string): Promise<WorkbenchState>`

Both parse `WorkbenchStateSchema`.

### Store

Add:

- `undoVersion(): Promise<void>`
- `restoreVersion(versionId: string): Promise<void>`

On success, replace store state with returned workspace state. On failure, append a visible system error using the existing error pattern.

### UI

- TopBar Undo is enabled when there are at least two version history items and no pending proposal/request busy state.
- HistoryPanel renders a Restore action for version history rows that are not the current version.
- Restore action closes or keeps the panel open only after state update; first version keeps it open to show the new history item.

## Product Requirement Mapping

| Requirement | Implementation Area | Verification |
| --- | --- | --- |
| PRD-01 | `top-bar.tsx`, `app.tsx`, `store.ts`, version route | Web test asserts Undo API call |
| PRD-02 | `run-panels.tsx`, `app.tsx`, `store.ts` | Web test asserts Restore API call |
| PRD-03, PRD-04 | `version_routes.rs`, store version creation | Server tests inspect version source/history |
| PRD-05, PRD-06 | `workspace_state_value`, returned graph payload | Server tests compare graph params before/after |
| PRD-07 | `proposal_routes.rs` conflict mapping | Server route test for stale proposal apply |
| PRD-08 | Store error handling | Existing visible system-error path |

## Risks

- Reusing target graph file assumes graph files are immutable once versioned. Current proposal apply and workspace creation already treat them this way.
- `parent_id` is a single link, so it records the operation source but does not encode a full restore target relation separately.
- History list ordering currently follows insertion order plus run/proposal entries. Restore entries will appear as normal version rows.

## Verification Commands

```sh
cargo fmt --check
cargo check --workspace
cargo test -p helixflow-server version_routes
cargo test -p helixflow-server proposal_routes
cargo test --workspace
cd web && npm test -- app.test.tsx
cd web && npm run build
git diff --check
```

## Rollback Plan

- Remove undo/restore routes and frontend actions.
- Return TopBar Undo to disabled placeholder and remove History restore buttons.
- Keep existing version records/table unchanged; no migration rollback is needed.
