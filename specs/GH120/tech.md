# Tech Spec

## Linked Issue

GH-120

## Product Spec

见 `product.md`。

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| HTTP handler | `crates/server/src/canvas_collaboration.rs` | `base_seq` 未使用；读取并覆盖 `comments.json` | CAS、幂等、409 映射与兼容导入的主要接线点 |
| Store API | `crates/store/src/canvas_comment_records.rs`、`crates/store/src/lib.rs` | 尚无 comments 持久化接口 | 集中实现 transaction/CAS，避免 server 拼接 SQL |
| SQLite schema | `crates/store/migrations/0005_canvas_comments_cas.sql` | comments 不在 SQLite | 建立 workspace state 与 operation 唯一约束 |
| Rust tests | 上述模块内测试 | 只有单请求 comments 与 presence 测试 | 增加 40 路并发、幂等、迁移和回归证明 |

## 设计方案

新增 `canvas_comment_states`，每个 workspace 一行，保存 `seq`、规范化
`comments_json`、`migration_source` 与更新时间。新增 `canvas_comment_operations`，以
`(workspace_id, operation_id)` 为主键，保存 operation fingerprint 与最终
`committed_seq`。

Store 暴露 comments 专用接口：初始化 state、读取 snapshot、原子提交。原子提交流程在单个
SQLite transaction 中先尝试插入 operation 行，以该写操作取得 SQLite writer serialization；
若 operation 已存在，则校验 fingerprint 后返回 replay。新 operation 随后读取当前 state 与
workspace 当前 version sequence，计算逻辑 `currentSeq`。显式 expected sequence 不匹配时
rollback 并返回 stale；匹配时更新 snapshot/sequence、补齐 operation 的 `committed_seq` 并
commit。预期冲突使用结果枚举返回，不经过 500 错误路径。

`CanvasCommentOpRequest` 增加可选 `operationId`，保持 `deny_unknown_fields` 与 camelCase
边界。服务端在调用 Store 前完成输入校验、规范化 comment op、生成 next snapshot 和稳定
fingerprint。未传 `operationId` 时生成 UUID；未传 `baseSeq` 时以 transaction 中的当前
sequence 提交，以兼容现有 Web 请求。

旧 JSON 兼容由 server 在 state 不存在时显式执行：读取、完整反序列化并校验旧 schema，随后
通过 Store 的 insert-if-absent 初始化。缺文件初始化为空 state；文件损坏直接失败。SQLite 行
存在后只读取 SQLite，不检查或回退旧文件。数据库 URL 与 `AppState` 的规则一致：优先
`HELIXFLOW_DATABASE_URL`，否则使用 `data_dir/helixflow.sqlite`。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 | state migration + atomic store commit | store transaction tests；server reload test |
| P2 | Store CAS result + `ApiError::conflict_with_details` | stale base 返回 409/currentSeq 且 snapshot 未变 |
| P3 | operation PK + fingerprint/committed_seq | 同 key 重放一次；不同 payload 冲突 |
| P4 | request optional fields + transaction current seq | legacy request add/patch/delete 回归测试 |
| P5 | server 40 路 Tokio concurrency test | 第一轮 1 success/39 conflict；刷新重试最终 40 |
| P6 | existing comments/presence tests | focused server suite |
| P7 | explicit legacy import | valid import once、post-import file mutation ignored、invalid JSON error |

## 数据流

1. Handler 验证 workspace、request、actor、target 与 body。
2. 若 SQLite state 不存在，读取并校验 legacy JSON，然后 insert-if-absent；并发初始化由
   workspace 主键收敛。
3. Handler 读取 SQLite snapshot，在内存副本上应用操作并生成 fingerprint。
4. Store transaction 按 operation 唯一键和 expected sequence 判定 replay、冲突或提交。
5. 成功或 replay 后从 SQLite 生成现有 Canvas response；stale/operation reuse 映射为
   409 JSON `{ "error": ..., "currentSeq": n }`。
6. Presence 继续只发布 `canvas.presence` event，不进入 comments 表。

## 备选方案

- 继续写 JSON 并加进程内 mutex：无法跨进程保证原子性，也不能提供 durable operation 唯一约束，拒绝。
- 每条评论完全规范化为多表：会扩大 migration 与查询范围；当前 snapshot + transaction 已覆盖
  GH-120，后续若需要评论查询再独立演进。
- 强制所有旧客户端立即提供 `operationId`/`baseSeq`：会破坏现有 UI，采用可选迁移字段。

## 风险

- Security: 所有 SQL 使用 bind 参数；workspace 外键隔离 operation，禁止动态 SQL。
- Compatibility: legacy 请求仍可提交；显式 `operationId` 才提供网络重试去重承诺。
- Performance: 同 workspace durable comments 写入串行化；snapshot JSON 重写为 O(n)，范围与现状
  相同且不扩大到 graph。
- Maintenance: `crates/store/src/lib.rs` 只增加 comments 模块声明/导出；具体接口集中在新文件，
  handoff 明确这是可能与其他 lane 接触的共享点。

## 测试计划

- [ ] Unit tests: Store CAS、stale rollback、operation replay/reuse、并发初始化。
- [ ] Integration tests: server 40 路 same-base 刷新重试、legacy import、add/resolve/delete、presence。
- [ ] Manual verification: 保存 red/green concurrency 日志并核对无 500、最终 40、sequence +40。
- [ ] Build: `cargo check --workspace`。
- [ ] Full tests: `cargo test --workspace`。
- [ ] Specs: GH120 focused 与 all-specs checks。

## 回滚方案

回滚 server/store code 与 `0005` migration 的使用，不做破坏性 down migration。旧
`comments.json` 未删除，可供旧版本继续读取；新版本在 SQLite 导入成功后不会修改旧文件。
若回滚后再次升级，operation/state 表保留并由新版本继续读取。
