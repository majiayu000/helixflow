# Product Spec

## Linked Issue

GH-91

## 用户问题

后端已有 `run_request` 与 `artifact_attach` 基础，但用户还不能从 canvas 完成
可见的 run -> artifact 回填流程。生成结果如果不能回到 canvas，canvas-agent
体验仍然断裂。

## 目标

- Canvas Run action 发出 `run_request`。
- 保留现有 cost gate 和确认 modal。
- 从 run events 和 accepted canvas ops 更新 node runtime。
- 在对应 canvas nodes 上渲染 attached artifacts。
- 复用现有 artifact preview metadata。
- 确保 image/video/text/json artifacts 可从 canvas inspect。

## 非目标

- 不替换 `RunService` 或 provider abstraction。
- 不新增 provider integration。
- 不改变 cost ledger 策略。

## Behavior Invariants

1. 用户可从 canvas 发起 run 并进入现有 cost confirmation。
2. Run step events 更新对应 canvas node runtime state。
3. `artifact_attach` 重试不会重复 artifact ids。
4. 生成 artifact 可在 canvas 上可见并可 inspect。
5. Reload 后 result state 和 artifact attachment 仍存在。

## 验收标准

- [ ] User can run from canvas, approve, and see node runtime update.
- [ ] Artifacts attach to originating nodes after run completion.
- [ ] Result state persists after reload.
- [ ] Duplicate `artifact_attach` retries do not duplicate artifact ids.

## 边界情况

- Run 失败时 node runtime 显示失败状态和错误信息。
- Cost gate 拒绝或取消时不能写入成功 runtime。
- artifact metadata 缺失时显示明确空状态，不静默伪造 preview。

## 发布说明

该变更连接 canvas 与现有 run/artifact 系统，不新增 provider。
