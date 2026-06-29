# Workbench Version Undo And Restore

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/24
Locale: zh-CN

## 背景

工作台已经能显示版本与运行历史，并且 proposal apply 会创建新的 server-owned graph version。原型中的 Undo 和 History restore 还没有接到真实版本系统，用户无法从错误 apply 或旧版本快速回退。

## 目标

1. 用户可以从 TopBar Undo 回退最近一次可撤销的 current version。
2. 用户可以从 History panel restore 指定历史 version。
3. Undo/restore 不删除历史记录，而是创建新的可审计 `restore` version，并让 workspace current 指向新 version。
4. Undo/restore 后 canvas 和 `workflowGraph` 使用恢复后的 graph。
5. 如果 pending proposal 基于旧 current version，恢复或撤销后 apply 必须返回明确 conflict。

## 非目标

- 不实现 redo。
- 不实现 graph diff viewer。
- 不改变 proposal dismiss contract。
- 不删除旧 version、run、proposal 或 artifact 记录。
- 不实现跨 workspace restore。

## 用户场景

### 场景 1：撤销最近一次应用

用户应用 proposal 后发现结果不对，点击 TopBar Undo。系统创建一条 `restore` version，canvas 回到上一版 graph，History 中能看到 restore 记录。

### 场景 2：恢复指定历史版本

用户打开 History panel，点击某个旧 version 的 Restore。系统创建新的 current version，graph 与被选中的历史 version 一致，刷新页面后仍保持恢复结果。

### 场景 3：pending proposal 已过期

用户收到 pending proposal 后执行 Undo 或 Restore。此时 pending proposal 的 `baseVersionId` 不再等于 current version；用户再 Apply 时必须得到 conflict，不允许把旧 proposal 应用到新 current graph。

## 产品需求

| ID | Requirement |
| --- | --- |
| PRD-01 | TopBar Undo 必须连接真实后端 version undo API。 |
| PRD-02 | History panel 必须能 restore 指定 version。 |
| PRD-03 | Undo/restore 必须创建新 version，`source` 为 `restore` 或等价可审计记录。 |
| PRD-04 | Undo/restore 不得删除或改写旧 history。 |
| PRD-05 | Undo 后 canvas 使用上一版 graph。 |
| PRD-06 | Restore 指定 version 后刷新仍保持 restored current version。 |
| PRD-07 | Pending proposal 基于旧版本时 apply 返回 conflict。 |
| PRD-08 | 请求失败时必须显示用户可见错误。 |

## 验收标准

- `POST /api/workspaces/{workspace_id}/versions/undo` 创建 restore version 并返回最新 workspace state。
- `POST /api/workspaces/{workspace_id}/versions/{version_id}/restore` 创建 restore version 并返回最新 workspace state。
- Undo/restore 后 `workspace.versionId` 是新 version id，`workflowGraph` 与目标 version graph 一致。
- History 中保留旧 version，并显示 restore 来源。
- 对已 stale 的 pending proposal 调用 Apply 返回 `409 Conflict`。
- Server/web tests 覆盖 undo、restore、history restore action 和 stale proposal conflict。

## 开放问题

1. Undo 按 current version 的 `parent_id` 优先，还是严格按 version index 的上一条记录？
2. Restore 当前 version 是否应该 no-op，还是也创建审计记录？
