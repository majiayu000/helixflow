# Product Spec

## Linked Issue

GH-105 (#105)

## 用户问题

Workbench 已有 manual edit session、Queue lock reason 和 `forceRerun` API，但跨 workspace 导航仍缺少完整的数据完整性边界：旧 workspace 的异步响应或 event 可能在切换后覆盖新 workspace；带未提交编辑的导航没有统一 Commit / Discard / Cancel 决策；聊天发送失败可能清空用户草稿；view mode 的交互限制也没有形成可测试的统一契约。

## 目标

- 所有 workspace-scoped 异步结果只允许更新发起时的 workspace generation。
- 有 dirty edit session 时，workspace switch、undo、restore、new workspace 必须先走 Commit / Discard / Cancel。
- 失败发送保留聊天草稿，只在服务端成功后清空。
- view mode 只允许 select、pan、zoom；move/connect/delete/paste 必须 fail closed。
- 保留并验证现有 Queue `forceRerun` 端到端契约，不重复实现。

## 非目标

- 不重做 GH72 的视觉设计或 manual edit 数据模型。
- 不引入新的后端 workspace API、协作协议或 provider。
- 不实现浏览器级离线队列。
- 不把导航确认扩展为自动保存；Commit 失败时必须停留并保留编辑。

## Behavior Invariants

1. workspace A 发起的 snapshot/canvas/message/provider 请求，在当前 workspace 已变为 B 时不得修改 B。
2. websocket event 的 `workspace_id` 与当前 workspace 不一致时不得修改当前 state。
3. dirty navigation 必须返回明确决策；Cancel 不切换、不清编辑，Discard 才丢弃，Commit 成功后才继续。
4. Commit 失败时导航取消，dirty ops 和错误保持可见。
5. chat draft 在 send pending 和 send failure 期间保留；成功响应后清空。
6. view mode 不产生 graph mutation、draft position/size、connection 或 clipboard paste op。
7. `forceRerun=true` 继续发送到 Queue API，后端 run 记录保持 force semantics。
8. 所有拒绝路径必须有可见原因，不静默 no-op。

## 验收标准

- AC1：A→B 切换后，迟到的 A snapshot/canvas/event/message response 不改变 B。
- AC2：dirty workspace switch 显示 Commit / Discard / Cancel；三条路径有集成测试。
- AC3：undo、restore、create workspace 复用同一 dirty navigation guard。
- AC4：send 失败后 input 仍含原草稿并显示错误；重试成功后才清空。
- AC5：view mode 下 drag/connect/delete/paste 不调用 mutation callback、不产生 dirty op。
- AC6：`forceRerun` 前后端现有测试继续通过。
- AC7：Web 全量 tests 与 production build 通过。

## 边界情况

- 同一 workspace 的连续 refresh：只接受最新 generation。
- workspace 切换后又快速切回：旧 generation 仍作废，不能凭 ID 相同复活。
- Commit 导航期间再次点击：只允许一个 pending navigation。
- IME composition 中 Enter：不得触发发送或清草稿。
- event seq 是 per-run，不得用另一个 workspace/run 的 seq 推进当前 state。

## 发布说明

Workbench 导航现在会保护未提交编辑；跨 workspace 的迟到响应不会再覆盖当前工作区；聊天发送失败会保留草稿；view mode 严格只读。
