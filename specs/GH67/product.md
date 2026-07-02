# Product Spec

## Linked Issue

GH-67

## 用户问题

agent 当前单轮调用 Codex CLI,一次性写 `out/proposal.json`。复杂图修改若首轮输出无效,系统直接把错误交给用户,agent 无法读取校验错误并自我修正。

## 目标

- agent turn 内部形成最多 N 轮的校验-回喂-重试循环,默认 3。
- 首轮 proposal 校验失败时,把结构化错误和当前图状态回喂给 Codex CLI。
- 校验通过后才交付用户评审。
- 每轮进度和错误在 runtime log/status 中可见。
- 超限失败时返回最后一轮校验错误。

## 非目标

- 不切换到直接 LLM API/tool calling。
- 不升级意图分类。
- 不让 agent 直接 mutate graph,仍走 proposal/review 流程。

## Behavior Invariants

1. 每轮 Codex CLI 输出都必须经过同一 proposal validation,不得因为重试跳过校验。
2. 校验失败回喂必须包含结构化错误、失败 opIndex/field、当前 compact graph/context。
3. 默认最多 3 轮;达到上限后 turn 失败并展示最后错误。
4. 任一轮通过校验后立即停止重试,持久化 pending proposal 供用户评审。
5. runtime log/status 显示每轮开始、失败原因、重试次数和最终结果。
6. 用户 interrupt/cancel 时停止后续 Codex CLI 调用并收敛状态。

## 验收标准

- [ ] 构造首轮必产坏图场景,agent 在 ≤3 轮内自愈并交付合法 proposal。
- [ ] runtime log 可看到每轮错误和重试。
- [ ] 超限失败信息包含最后一轮校验错误。

## 边界情况

- Codex CLI 进程失败或超时。
- 第二轮输出格式无效 JSON。
- 用户在重试中断 run。

## 发布说明

agent 仍由 Codex CLI 承载,但复杂修改会更像“自动修正后交付”。需要在诊断信息中显示重试轮次。

