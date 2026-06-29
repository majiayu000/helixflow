# Output Selection And Artifact Preview

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/25
Locale: zh-CN

## 背景

工作台已经能在 run 完成后返回 artifacts，并有 OutputsStrip 和 ArtifactStage 的组件雏形。但 output selection 还不是 server-owned 行为，ArtifactStage 也没有接入主工作台。用户选择某个 output 后，刷新页面不能可靠保持选择状态。

## 目标

1. 用户可以从 OutputsStrip 选择一个 output。
2. 选择状态由后端 artifact record 持久化，刷新 workspace state 后仍保留。
3. ArtifactStage 显示 selected output 的安全预览；没有可预览内容时保持空状态。
4. Workspace state 只包含轻量 preview contract，不把大 artifact 内容塞进 state JSON。
5. 前端不得看到本地绝对路径或 provider 内部存储路径。

## 非目标

- 不实现 seed sweep recommendation UI。
- 不允许未审查 HTML/JS 获得宿主页权限。
- 不把 provider storage details 暴露给前端。
- 不实现完整 artifact binary storage backend。
- 不改变 run execution 或 artifact creation semantics。

## 用户场景

### 场景 1：选择 output

Run 产生多个 artifacts。用户点击 OutputsStrip 中的某个 output，UI 标记它为 selected，ArtifactStage 显示该 output 的 preview。刷新页面后仍显示同一个 selected output。

### 场景 2：安全预览

选中的 artifact 是 text/html/image/video。工作台只显示安全预览或轻量 summary。HTML 预览必须被 sandbox，不能获得宿主页权限；image/video 不把大二进制塞进 state JSON。

### 场景 3：没有 preview

如果 selected output 没有可安全预览内容，ArtifactStage 保持空状态，OutputsStrip 仍显示 artifact 列表和 selected 状态。

## 产品需求

| ID | Requirement |
| --- | --- |
| PRD-01 | Output selection 必须调用后端 API，不能只改前端本地 state。 |
| PRD-02 | 同一个 run 中只能有一个 selected output。 |
| PRD-03 | 选择后刷新 workspace state 仍保留 selected output。 |
| PRD-04 | ArtifactStage 使用 selected output，不自行猜测业务真相。 |
| PRD-05 | Workspace state 不得包含大 artifact 内容。 |
| PRD-06 | `storageUri` 或 download contract 不得暴露本地绝对路径。 |
| PRD-07 | HTML preview 必须 sandbox，不能获取宿主页权限。 |
| PRD-08 | 选择不存在或不属于当前最新 run 的 output 时返回明确错误。 |

## 验收标准

- `POST /api/outputs/{id}/select` 持久化 selected artifact，并返回最新 workspace state。
- 同一 run 中其他 artifacts 被取消 selected。
- Refresh workspace state 后 selected output 仍是刚选择的 output。
- ArtifactStage 渲染 selected output 的 preview。
- `storageUri` 使用安全 download route，不暴露 `/Users/...`、`/tmp/...`、Windows drive path 等本地绝对路径。
- Server/web tests 覆盖 selection persistence、latest-run guard、safe payload 和 ArtifactStage 接线。

## 开放问题

1. 未来是否需要单独的 artifact binary storage service？
2. image/video preview 是第一版 summary，还是后续接入 signed URL/thumbnail route？
