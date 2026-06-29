# Seed Sweep Run Plans And Recommendations

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/28
Locale: zh-CN

## 背景

工作台已经有 cost gate、pending confirmation、manual run、output selection 和 artifact preview。Seed 试验 preset 现在会进入 `RunRequest`，但用户还看不到一次 sweep 的 run count、总费用、pending changes 和推荐输出；确认后也没有通过同一个确认动作执行整组 run 并把推荐 output 选中。

## 目标

1. 用户请求 seed sweep 时，系统生成受校验的 sweep run plan。
2. 用户在 ConfirmModal 中看到 run count、cost estimate、pending changes 和可中断性。
3. 确认前不得执行 provider invoke；确认后一次确认执行多个 sweep runs。
4. sweep 完成后 outputs state 包含多个 outputs，并把推荐 output 标记为 selected。
5. cost estimate 和 actual cost 都能从 ledger 审计。

## 非目标

- 不接入真实付费 provider。
- 不绕过用户确认。
- 不实现 arbitrary batch scripting。
- 不实现复杂 best-output AI 评审；第一版使用 deterministic recommendation。

## 用户场景

### 场景 1：确认 seed sweep

用户点击 Seed 试验 preset。Agent 回复一个待确认 run request。ConfirmModal 显示本次 sweep 会创建几个 runs、每个变体的 seed 变更、总费用估算，以及确认后可中断。

### 场景 2：执行并推荐 output

用户确认后，系统按 sweep plan 逐个执行 run。完成后 OutputsStrip 显示多个 artifacts，其中 deterministic recommendation 被标记为 selected，ArtifactStage 显示该推荐 output。

### 场景 3：取消或中断

用户在确认前取消，整组 sweep runs 都不会调用 provider。执行期间中断当前 active run 时，剩余 sweep steps 不继续执行。

## 产品需求

| ID | Requirement |
| --- | --- |
| PRD-01 | Seed sweep 请求必须产生 `sweep` trigger 的 run plan，且每个 variant 都通过 graph schema/registry 校验。 |
| PRD-02 | ConfirmModal 必须显示 run count、总 cost estimate、pending changes 和 interruptible 状态。 |
| PRD-03 | 未确认前不得调用 provider invoke。 |
| PRD-04 | 单次确认必须执行同一个 sweep group 中的多个 runs。 |
| PRD-05 | 确认后 outputs state 必须包含多个 outputs。 |
| PRD-06 | 系统必须持久化推荐 output 的 selected 状态。 |
| PRD-07 | estimate cost 和 actual cost 必须分别写入 cost ledger。 |
| PRD-08 | 取消确认必须取消整组 pending sweep runs。 |
| PRD-09 | 执行中断必须停止剩余 sweep steps。 |

## 验收标准

- Seed 试验消息返回 `pendingConfirmation.runCount=4`，并包含 seed pending changes。
- ConfirmModal 渲染 run count、pending changes、total estimate 和 interruptible 文案。
- 确认前所有 sweep runs 保持 `waiting_confirmation`，没有 output artifacts。
- `POST /api/workspaces/{workspace_id}/runs/{run_id}/confirm` 对 sweep run id 执行整组 sweep，并返回多个 outputs。
- 返回 outputs 中只有推荐 output `selected=true`。
- `cost_ledger` 中同时存在 estimated 和 actual entries。
- `POST /api/workspaces/{workspace_id}/runs/{run_id}/hold` 对 sweep run id 取消同组所有 waiting runs。

## 开放问题

1. 未来是否需要 provider-side scoring 来替代 deterministic recommendation？
2. UI 是否需要单独展示每个 variant 的进度，而不是复用现有 RunDock？
