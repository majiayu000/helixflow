# Product Spec

## Linked Issue

GH-67

## 用户问题

agent 当前每次只执行一轮 `codex exec`,写出 proposal 后立即读取校验。复杂图修改如果首轮输出非法 proposal,错误直接暴露给用户,agent 没机会读取结构化校验错误并自我修正。

## 目标

- Modify/Debug workflow 模式下,agent turn 内部形成最多 N 轮循环,默认 3。
- 每轮输出 proposal 后,后端校验;失败时把错误、当前图和上一轮摘要回喂给 Codex CLI 再试。
- 只有校验通过的 proposal 才交付用户评审。
- 每轮进度和错误通过现有 agent status/log 通道可见。
- 超限失败时,用户看到最后一轮校验错误和明确失败原因。

## 非目标

- 不改为直接 LLM API 或通用 tool-calling 框架。
- 不升级意图分类。
- 不自动 apply proposal,仍需要用户评审。

## Behavior Invariants

1. agent 首轮 proposal 校验失败时,系统不会立即向用户交付 invalid proposal,而是进入下一轮重试。
2. 重试 prompt 包含结构化校验错误、失败 proposal 摘要和当前 graph context,但不泄露 secrets、本地 unrestricted path 或隐藏 ctx。
3. 默认最多 3 轮;达到上限仍失败时,agent 返回 failed 状态,消息包含最后错误。
4. 任一轮通过 `read_validated_proposal` 和 `GraphService::preview_proposal` 后,循环停止并交付 pending proposal。
5. 每一轮都有 runtime log/status:round started、validation failed、retrying、proposal ready 或 exhausted。
6. Chat 模式不进入 proposal retry loop。

## 验收标准

- [ ] 构造首轮输出非法 node_type 的 fake runtime,agent 在后续轮次自愈并交付合法 proposal。
- [ ] runtime log 中可见每轮错误和 retry 状态。
- [ ] 超过 N 轮后失败信息包含最后校验错误。

## 边界情况

- Codex CLI 进程失败不计为可校验 proposal,但仍消耗一次 round 并进入 retry/失败逻辑。
- base graph 在循环期间不变;若外部版本变更,最终提交时走既有 stale proposal gate。
- retry prompt 不能追加无限历史;只保留当前图、最后错误和必要摘要。

## 发布说明

默认轮次为 3,可通过配置调整。继续使用 Codex CLI runtime。
