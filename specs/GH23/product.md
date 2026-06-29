# Workbench Run Queue And Interrupt Controls

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/23
Locale: zh-CN

## 背景

Helixflow 已经有 `RunService`、WebSocket progress、agent-requested confirmation 和前端 run dock。GH21 PR #22 会恢复 ComfyUI Agent 工作台壳子，并保证“运行当前 workflow”不会被误分类成 graph redesign。

但工作台仍缺少一个直接、可审计的手动运行入口。用户现在容易被迫通过 chat 输入“运行当前 workflow”，这会把一个明确的 workbench action 混进 agent turn routing。原型中的 Queue / Interrupt 需要接到真实后端，而不是前端 demo state。

## 目标

1. 用户可以从工作台按钮直接 Queue 当前 workflow。
2. 用户可以中断正在执行的 active run。
3. `waiting_confirmation` 的 hold/approve 和 active run 的 interrupt 行为保持区分。
4. 重复点击 Queue 或 Interrupt 不会造成重复执行、重复 provider invocation 或错误的成功状态。
5. 所有状态变化来自 server-owned run state，前端不自行模拟 run lifecycle。

## 非目标

- 不实现 undo/restore。
- 不实现 workflow JSON export。
- 不实现 output selection 或 ArtifactStage。
- 不实现 seed sweep。
- 不实现 failed-run diagnosis card。
- 不改变 provider secret、BYOK 或 runtime provider 配置。
- 不让 agent 直接调用 provider API。

## 用户场景

### 场景 1：从工作台运行当前 workflow

用户已经有一个可执行 workflow，点击 Queue。系统创建一个真实 run，run dock 显示 queued/running/succeeded/failed/interrupted 状态，canvas 节点状态通过 WebSocket 更新。

### 场景 2：当前 workflow 不可运行

用户点击 Queue，但当前 graph 没有可编译 execution plan 或缺少必要节点。系统返回明确错误，不能静默成功，也不能创建假 artifact。

### 场景 3：中断 active run

用户看到 run 正在执行，点击 Interrupt。系统请求中断该 active run，未执行的步骤被跳过，run 最终进入 `interrupted`。如果 run 已经结束，系统返回明确错误或保持已结束状态，不显示成功中断。

### 场景 4：等待确认的 run

agent-requested run 处于 `waiting_confirmation` 时，用户仍通过 confirmation modal approve/hold。Interrupt 不应替代 hold，hold 也不应伪装成 active-run interrupt。

## 产品需求

| ID | Requirement |
| --- | --- |
| PRD-01 | 工作台必须提供直接 Queue 当前 workflow 的 UI action。 |
| PRD-02 | Queue action 必须调用后端 run API，不能通过发送 chat message 实现。 |
| PRD-03 | active run 必须提供 Interrupt action。 |
| PRD-04 | `waiting_confirmation` run 继续由 confirm/hold 控制，不能被误当作 active run interrupt。 |
| PRD-05 | Queue/Interrupt 请求失败时必须向用户显示明确错误。 |
| PRD-06 | 前端 button disabled/loading 状态必须防止同一用户操作造成明显重复请求。 |
| PRD-07 | 后端必须防止并发请求造成同一 run 双执行。 |
| PRD-08 | Run state、steps、artifacts 和 node status 必须来自后端 state/event。 |

## 验收标准

- 点击 Queue 后，后端创建真实 run，并返回最新 run payload。
- Queue 不通过 `POST /api/workspaces/{id}/messages` 或 agent prompt routing 触发。
- active run 点击 Interrupt 后，run 最终显示 `interrupted`，未开始 step 显示 skipped 或等价非成功状态。
- 对 finished run 调用 Interrupt 不会把 succeeded/failed run 改成 interrupted。
- `waiting_confirmation` run 的 Hold/Approve 行为保持不变。
- 快速重复点击 Queue/Interrupt 不会造成 provider double invoke。
- `cargo test -p helixflow-run`、`cargo test -p helixflow-server run_routes` 和 `cd web && npm test -- app.test.tsx` 覆盖主要路径。

## 开放问题

1. 第一版是否允许同一 workspace 同时存在多个 manual active runs，还是需要返回 conflict？
2. Queue button 的文案是否保留原型的 `Queue`，还是中文显示为“运行”？
3. active run interrupt 失败时，UI 是 toast/error banner，还是写入 chat/system message？
