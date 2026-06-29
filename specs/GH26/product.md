# Workbench Workflow JSON Export

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/26
Locale: zh-CN

## 背景

工作台 TopBar 已经保留 Export 入口，但它不能导出前端临时 canvas state，也不能在存在 pending proposal 时误导出 preview graph。导出必须来自 server-owned current version，用户拿到的 JSON 才能和已应用版本一致、可审计、可复现。

## 目标

1. 用户可以从 TopBar 导出当前已应用 workflow JSON。
2. 默认导出 workspace 的 current version，不导出 pending proposal preview。
3. 导出 payload 必须符合 `WorkflowGraph` JSON schema。
4. 导出内容不得包含本地绝对路径、provider secrets、auth headers 或 runtime-only metadata。
5. 导出失败时必须给用户明确错误，不能静默下载错误内容。

## 非目标

- 不实现 ComfyUI native format converter。
- 不实现 pending preview export。
- 不实现 import。
- 不实现 undo/restore。
- 不实现 output selection 或 artifact preview。

## 用户场景

### 场景 1：导出当前版本

用户已经应用了一个 workflow version，点击 TopBar Export。浏览器下载一个 JSON 文件，内容是后端当前 version 的 workflow graph。

### 场景 2：存在 pending proposal

用户收到一个 pending proposal，但尚未点击 Apply。点击 Export 时，系统仍导出 current version graph，而不是 proposal preview graph。

### 场景 3：导出内容不可公开

如果 stored graph 中包含疑似 secret、auth header、本地绝对路径或 runtime-only metadata，导出请求返回明确错误，不生成包含敏感数据的下载。

## 产品需求

| ID | Requirement |
| --- | --- |
| PRD-01 | TopBar 必须提供可点击的 workflow JSON export action。 |
| PRD-02 | Export action 必须调用后端 version export API，不能从前端 canvas state 或 pending proposal preview 拼 JSON。 |
| PRD-03 | 默认导出 workspace current version。 |
| PRD-04 | 有 pending proposal 时，默认导出的仍是 current graph。 |
| PRD-05 | 导出 payload 必须通过 `WorkflowGraph` schema 校验。 |
| PRD-06 | 导出内容不得包含本地绝对路径、provider secrets、auth headers 或 runtime-only metadata。 |
| PRD-07 | 导出失败时必须显示用户可见错误。 |

## 验收标准

- 点击 Export 会请求 `GET /api/versions/{version_id}/export` 或等价 server-owned endpoint。
- 后端返回当前 version graph JSON。
- 有 pending proposal 时，导出的 graph 仍是 current version graph。
- 前端下载内容来自后端响应，并通过 `WorkflowGraphSchema` 校验。
- Unsafe graph export 返回错误，不把 secret/path/header 字段写入响应。
- `cargo test -p helixflow-server version_routes` 和 `cd web && npm test -- app.test.tsx` 覆盖主要路径。

## 开放问题

1. 第一版下载文件名是否需要包含 workspace name、version id 和时间戳？
2. Unsafe graph 是否长期采用 reject 策略，还是未来提供显式 redaction report？
