# Product Spec

## Linked Issue

GH-120

## 用户问题

Canvas comments 当前忽略 `baseSeq`，并通过无锁 JSON read-modify-write 覆盖持久化。
并发协作者会收到成功响应，但已确认的评论仍可能消失；文件写冲突还会暴露为 500，
使 sequence 无法代表真实提交顺序。

## 目标

- 让同一 workspace 的评论变更具有原子并发控制，不丢失已确认提交。
- 为冲突提供稳定的 409 响应和当前 sequence，允许调用方刷新后重试。
- 用稳定 `operationId` 保证同一操作的网络重试不会重复生效。
- 将既有 comments JSON 显式、一次性迁移到 SQLite，迁移后 SQLite 是唯一真相源。

## 非目标

- 不修改 graph、layout、proposal、version 或 run 的并发模型。
- 不改动 Web UI 或替 Web 客户端实现自动冲突重试。
- 不改变 presence 的临时广播语义或 WebSocket 事件协议。
- 不实现跨 workspace 的评论事务。

## Behavior Invariants

1. `P1`：每个 workspace 的 durable comments snapshot 与 comment sequence 必须存储在
   SQLite；一次成功提交必须原子地更新二者，服务端不得再把覆盖式 JSON 当作并发真相源。
2. `P2`：请求携带 `baseSeq` 时，服务端必须用 workspace + expected sequence 做原子
   CAS；stale base 必须返回 HTTP 409 和 camelCase `currentSeq`，不得返回 500，也不得
   部分写入评论、sequence 或 operation 记录。
3. `P3`：请求可携带稳定 camelCase `operationId`。同一 workspace 内，同一
   `operationId` 与同一规范化操作内容的重试只能提交一次；重放返回成功且不得重复新增、
   重复递增 sequence。同一 `operationId` 被不同内容复用必须返回 409 和 `currentSeq`。
4. `P4`：为兼容现有客户端，缺少 `baseSeq` 的请求仍可在服务端当前 sequence 上串行提交；
   缩略 schema 中缺少 `operationId` 时服务端生成一次性 ID。该兼容路径不得丢更新，但只有
   调用方提供稳定 `operationId` 时才承诺跨网络重试去重。
5. `P5`：40 个携带同一有效 `baseSeq`、不同稳定 `operationId` 的并发新增操作，第一轮
   只能有一个提交，其余为 409；调用方使用每次 409 的 `currentSeq` 刷新并重试后，最终必须
   保留 40 条评论，无 500，comment sequence 恰好递增 40 次。
6. `P6`：单请求 add、edit、resolve/reopen、delete 的校验、响应和持久化行为保持有效；
   presence、WebSocket 广播与评论 durable transaction 相互独立。
7. `P7`：发现旧 `comments.json` 且 SQLite 尚无该 workspace 状态时，服务端必须显式导入
   一次并记录迁移来源。无效 JSON 或不合法 sequence 必须报错，不能静默使用空状态；导入
   成功后即使旧文件变化也不得回退读取。

## 验收标准

- [ ] 40 路 same-base 并发测试证明 409/currentSeq、无 500、无部分写，刷新重试后保留 40 条。
- [ ] 幂等测试证明同 `operationId` 重试只提交一次，复用不同 payload 返回 409。
- [ ] migration 测试覆盖 legacy JSON 一次性导入、导入后不回退和非法文件 fail-closed。
- [ ] focused Rust tests、`cargo check --workspace`、`cargo test --workspace` 全部通过。
- [ ] `python3 checks/check_workflow.py --repo . --spec-dir specs/GH120` 与
  `python3 checks/check_workflow.py --repo . --all-specs` 通过。

## 边界情况

- stale `baseSeq` 不得创建 operation 幂等记录，否则合法重试会被误判为已提交。
- operation replay 发生在其他评论已提交之后时，返回当前 snapshot，但不得再次执行旧操作。
- 评论状态尚未初始化时，并发请求只能完成一次空状态或 legacy JSON 初始化。
- graph version sequence 高于 comment sequence 时，CAS 的 `currentSeq` 继续与现有 Canvas
  顶层 sequence 对齐，避免合法 Canvas snapshot 被误判。

## 发布说明

服务端 migration 随 SQLite schema 自动执行。旧 JSON 仅在对应 workspace 的 SQLite comment
state 不存在时导入一次；不会删除旧文件，以便回滚旧版本，但新版本导入后不再读取该文件。
`operationId` 是向后兼容的可选字段，建议客户端后续为每次用户操作生成并在重试间复用。
