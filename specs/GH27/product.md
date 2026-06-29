# Failed Run Diagnosis Cards And Minimal Fix Proposals

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/27
Locale: zh-CN

## 背景

工作台已经能把 run step 标为 `failed`，并且用户请求“修复报错”时可以进入 `DebugWorkflow`。但失败信息还没有以稳定的产品形态进入 workspace state，chat 中也没有 ErrorCard 让用户先看到人类可读摘要，再按需展开 raw error。

## 目标

1. failed run 后，用户能在 canvas 中看到 failed node。
2. chat 中显示 ErrorCard，默认展示人类可读摘要。
3. raw error 默认折叠，只有用户展开时显示。
4. 用户请求“修复报错”时进入 `DebugWorkflow`，agent 基于最近 failed run context 输出解释或最小 fix proposal。
5. fix proposal apply 前不得改变 current graph。

## 非目标

- 不实现自动重试。
- 不实现 seed sweep。
- 不让 agent 直接调用 provider。
- 不让 agent 读取 secret。
- 不改变现有 proposal apply/dismiss 审核路径。

## 用户场景

### 场景 1：运行失败后定位节点

用户运行 workflow，某个 provider step 失败。Canvas 保持该 node 的失败标记，chat 中显示失败摘要卡片，用户不用阅读内部日志也能知道失败节点和原因。

### 场景 2：查看 raw error

用户需要排查细节时，可以展开 ErrorCard 查看 raw error。raw error 不默认出现在 assistant 主回复或普通消息正文里。

### 场景 3：请求最小修复

用户点击或输入“修复报错”。系统进入 `DebugWorkflow`，agent 使用最近 failed run context，生成最小 graph fix proposal，或明确说明上下文不足不能修复。proposal 仍需用户应用后才改变 current graph。

## 产品需求

| ID | Requirement |
| --- | --- |
| PRD-01 | `WorkbenchState.run` 必须包含 latest failed run 的结构化 error 摘要。 |
| PRD-02 | `WorkbenchState.run.steps[]` 必须保留 failed step 的结构化 error 摘要。 |
| PRD-03 | ChatPane 必须在 latest run failed 时显示 ErrorCard。 |
| PRD-04 | ErrorCard 默认只显示 summary，raw error 默认折叠。 |
| PRD-05 | Canvas 必须继续使用 run step state 标出 failed node。 |
| PRD-06 | “修复报错”必须路由到 `DebugWorkflow`。 |
| PRD-07 | DebugWorkflow 必须使用最近 run error context。 |
| PRD-08 | fix proposal apply 前不得改变 current graph 或 current version。 |

## 验收标准

- workspace state 中 failed run 和 failed step 都有 `error.summary`，并保留可展开 raw。
- App 渲染 failed run 时 chat 区出现 ErrorCard，raw error 不默认渲染。
- WebSocket `node.state` / `run.failed` 能更新可见 failed state 和 error summary。
- “修复报错”走 `debug_workflow`，并能把 latest failed run context 交给 agent。
- DebugWorkflow response 只创建 pending proposal，不自动 apply。

## 开放问题

1. 未来是否需要 server-side secret redaction policy 覆盖所有 provider error？
2. ErrorCard 是否需要支持多 failed step 的分组视图？
