# Product Spec: Agent Bounded Canvas Ops Contract

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/47
Locale: zh-CN

## 背景

Helixflow 已有 Agent proposal、run confirmation、manual proposal 和 canvas selection/layout 能力。相比 infinite-canvas，当前缺口不是让 Agent 任意操作 UI，而是缺少一个面向 canvas 的有限操作合同，让 Agent 明确通过结构化状态读取、proposal diff 和 confirmation gate 工作。

## 目标

1. Agent 可以读取当前 graph 的 compact state。
2. Agent 可以读取当前 canvas selection 的 compact state。
3. Agent 对 layout 的建议必须表现为 `move_node` proposal，不得直接保存版本。
4. Agent 对 graph 的建议必须进入 pending proposal，不得绕过 apply/dismiss。
5. Agent 对运行请求必须进入 pending confirmation，不得直接执行 provider。
6. 工具/上下文证据必须作为独立日志显示，不污染主 assistant answer。
7. canvas ops 输入和输出合同必须 fail closed，unknown fields 不得被静默忽略。

## 非目标

- 不发布外部 MCP 或跨进程工具协议。
- 不允许 Agent 直接调用前端 UI action。
- 不允许 Agent 直接执行破坏性 graph mutation。
- 不暴露 secrets、本地绝对路径、provider raw config 或 auth headers。
- 不实现局部子图执行引擎；第一版运行仍使用当前 backend run confirmation 路径。

## 用户场景

### 场景 1: 读取当前画布状态

用户向 Agent 询问当前 workflow。Agent 读取 `ctx/canvas_state.json` 中的 compact graph 状态，而不是臆造节点、端口或 selection。

### 场景 2: 基于选择修改布局

用户选择多个节点后要求“把这几个节点排齐”。Agent 使用 selection context，并通过 `proposal.json` 输出 `move_node` ops。系统创建 pending proposal，用户 apply 前 current graph 不变。

### 场景 3: 基于选择修改 graph

用户选择节点后要求“给选中的 video 增加时长”。Agent 使用 bounded proposal ops 生成 pending proposal。已有 pending proposal 时必须拒绝新 proposal。

### 场景 4: 运行当前 workflow

用户要求运行当前 workflow。Agent 进入 run request 合同，backend 创建 pending confirmation。用户确认前不执行 provider。

### 场景 5: 查看工具证据

UI 主消息只显示用户可读结果；canvas ops context、prompt metadata 和 runtime status 归入可展开工具日志组。

## 产品需求

| ID | Requirement |
| --- | --- |
| PRD-01 | Agent graph 模式必须获得 compact canvas state 文件。 |
| PRD-02 | 前端必须把当前 selection 作为 bounded context 发送给 message API。 |
| PRD-03 | Canvas ops 合同必须包含 `read_state`、`read_selection`、`propose_layout`、`propose_graph_ops`、`run_selected_workflow`。 |
| PRD-04 | `propose_layout` 必须映射为 `move_node` proposal，不得调用 layout save API。 |
| PRD-05 | `propose_graph_ops` 必须沿用 existing proposal preview/apply/dismiss gate。 |
| PRD-06 | 已有 pending proposal 时，Agent proposal 模式必须 fail closed。 |
| PRD-07 | `run_selected_workflow` 第一版必须使用 pending confirmation，不得直接执行 provider。 |
| PRD-08 | Unknown canvas ops fields 必须被 schema 校验拒绝。 |
| PRD-09 | UI 必须把 canvas ops evidence 显示在工具日志区域。 |

## 验收标准

- Agent session context 里存在 `ctx/canvas_state.json` 和 `ctx/canvas_ops.json`。
- `canvas_state.json` 包含 compact graph 和当前 selection node ids。
- Agent prompt 明确 layout/graph/run ops 的 gate 映射。
- 已有 pending proposal 时，Agent 修改请求返回错误且不创建新 proposal。
- 前端发送消息时包含当前 selection context。
- UI 工具日志能显示 canvas ops evidence。
- Rust/TS tests 覆盖 schema unknown fields、selection context、proposal gate preservation 和 UI evidence。
