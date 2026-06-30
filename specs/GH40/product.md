# Runtime Provider Catalog And State

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/40
Locale: zh-CN

## 背景

M0-M2 已经把本地工作台主链路跑通：workspace、chat、proposal、apply、manual run、run confirmation、seed sweep、output preview、export、undo/restore 都有真实后端路径和 QA 台账。

M3 要继续接真实 provider、ComfyUI adapter 和更多 API connector。当前 provider truth 仍不够明确：

- server 默认只配置 `mock` provider；
- 未知 `HELIXFLOW_RUNTIME_PROVIDER` 会变成 unavailable provider，但 workspace state 没有把这个 provider truth 明确返回给前端；
- frontend 的 provider 信息仍可从 schema default 得到 `mock`；
- TopBar 仍有硬编码 Atlas 配置文案；
- agent graph modes 只拿到 node catalog，没有拿到 workflow backend、runtime provider、API connector catalog 文件。

在接真实 provider 前，系统必须先让用户、前端、agent 和 run layer 看到同一份后端拥有的 provider/catalog 状态，避免 mock/default 被误认为真实外部 provider。

## 目标

1. Workspace state 明确返回后端拥有的 provider catalog/status。
2. 前端 provider 状态只使用后端返回的数据，不再靠 mock default 或 Atlas 硬编码文案。
3. Agent graph modes 的 session context 包含 workflow backend、runtime provider、API connector 三类 catalog 文件。
4. 未配置 provider 必须显示为 unavailable，并阻止 run 以 mock 静默替代。
5. 默认本地开发仍能使用 clearly labeled `mock` provider 跑通。

## 非目标

- 不接入真实 Atlas、OpenAI、Replicate、Stability、Runware 或其他 provider API。
- 不实现 ComfyUI `/prompt` 调用。
- 不实现 BYOK / secret 管理 UI。
- 不实现 provider marketplace。
- 不改变 graph proposal、run confirmation、seed sweep 的现有用户流程。
- 不把 mock provider 移除；mock 仍用于本地开发和测试，但必须清晰标识。

## 用户场景

### 场景 1：默认本地开发

用户没有设置 `HELIXFLOW_RUNTIME_PROVIDER`，打开工作台。系统显示当前 runtime provider 是 `mock`，状态 healthy，并明确这是本地测试 provider。用户仍可创建 workflow 并运行 mock workflow。

### 场景 2：未知 provider 配置

用户设置 `HELIXFLOW_RUNTIME_PROVIDER=openai`，但当前 build 没有配置 OpenAI connector。工作台显示 `openai` unavailable，说明该 provider 未配置。用户不应看到 mock provider 被当作 fallback 的成功状态。

### 场景 3：Agent 创建 workflow

用户让 agent 创建 workflow。Agent session 的 `ctx/` 中包含 workflow backend、runtime provider、API connector catalog。Agent 可以知道当前可用 provider/capability 的边界，但不会收到 secrets。

### 场景 4：前端状态展示

用户查看 TopBar 或状态区域。UI 从后端 state 读取 provider label/status，不再显示硬编码 `Atlas 已配置` 或 `Atlas 未配置`。

## 产品需求

| ID | Requirement |
| --- | --- |
| PRD-01 | Workspace state 必须包含后端拥有的 provider catalog/status，不允许前端用本地 default 伪造 provider truth。 |
| PRD-02 | 默认配置必须返回 clearly labeled `mock` provider，并保持现有本地 run 能力。 |
| PRD-03 | 未知或未配置 provider 必须返回 unavailable 状态，且不得 fallback 到 mock 执行。 |
| PRD-04 | Agent graph modes 必须写入 workflow backend、runtime provider、API connector 三类 catalog 文件。 |
| PRD-05 | Agent context/catalog 不得包含 provider secrets、raw auth headers、signed URLs 或本地敏感路径。 |
| PRD-06 | 前端 provider 状态展示必须来自后端 state，不得硬编码 Atlas 配置状态。 |
| PRD-07 | 现有 chat、proposal、run confirmation、seed sweep、output preview、undo/restore 流程不得回退。 |

## 验收标准

- `GET /api/workspaces/{workspace_id}/state` 返回 provider catalog/status 字段。
- 默认环境下 state 显示 `mock` provider healthy，并且 existing mock workflow run 仍通过。
- `HELIXFLOW_RUNTIME_PROVIDER=openai` 这类未知配置显示 unavailable，并且 run 不会被 mock provider 代替执行。
- Agent graph session 包含：
  - `ctx/workflow_backends/catalog.json`
  - `ctx/runtime_providers/catalog.json`
  - `ctx/api_connectors/catalog.json`
- Agent context 文件不包含 secrets 或 raw auth material。
- 前端测试覆盖 provider status rendering，且源码中不再出现 `Atlas 已配置` / `Atlas 未配置`。
- 现有 Rust workspace tests 和 web tests 通过。

## 开放问题

1. 第一个真实 connector 应该优先做 local ComfyUI、Atlas，还是更通用的 HTTP connector？
2. Provider catalog 的 disabled/unavailable 示例是否应只展示当前 env 指定 provider，还是提前展示可安装 provider 列表？
3. Future BYOK UI 是否属于 provider catalog 扩展，还是单独的 credentials/settings issue？
