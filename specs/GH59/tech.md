# Tech Spec

## Linked Issue

GH-59

## Product Spec

`specs/GH59/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Op 模型 | `crates/graph/src/lib.rs:367` | `ProposalOp` 六种 op(AddNode/RemoveNode/SetParam/AddEdge/RemoveEdge/MoveNode);`GraphService::apply_ops` + `validate_graph` + `preview_proposal` 提供校验通道 | 统一后的唯一 op 模型,新端点直接复用 |
| 手动编辑双轨 | `crates/server/src/manual_proposal_routes.rs`, `crates/server/src/manual_proposal_routes_tests.rs` | `POST /proposals/manual` 单 op(`ManualProposalOpRequest` :34),经 `manual_op_to_proposal_op`(:175)转 `ProposalOp`,走 proposal → apply 两步 | 被新端点取代后整体删除;其 op 请求线格式与转换/校验辅助函数迁移复用 |
| 死代码双轨 | `crates/agent/src/canvas_ops.rs:101-116`, `crates/agent/src/tests.rs:197-210`, `crates/agent/src/lib.rs:19-21` | `CanvasOpsRequest`/`CanvasOp` 除一个拒绝未知字段的单测外零消费方 | 删除目标。注意同文件 `CanvasOpsContext`(:8,写入 ctx/canvas_state.json)与 `CanvasOpsContract`/`CanvasOpSpec`(:127/:161,经 `lib.rs:100` 写入 ctx/canvas_ops.json)均已接线,必须保留 |
| 即时成版先例 | `crates/server/src/layout_routes.rs` | `POST /versions/layout`(GH44):base version 乐观锁、pending proposal 拒绝、`apply_ops`+`validate_graph`、写图文件、`create_version_after` CAS 成版(source=manual) | 新端点复制该骨架并推广到全部 op 类型 |
| 版本记录与撤销 | `crates/store`(versions 表), `crates/server/src/version_routes.rs` | source 取值 manual/proposal/restore;undo/restore 路由生成 source=restore 版本;`create_version_after` 在 DB 层做当前版本 CAS | 手动即时成版的撤销兜底与并发第二道防线 |
| 错误响应 | `crates/server/src/api_error.rs` | `ApiError { status, message }`,JSON 体为 `{"error": message}`;`ApiError::store` 把 `StoreError::VersionConflict` 映射为 400 | 需扩展可选 `opIndex` 字段;ops 路由需将版本 CAS 冲突映射为 409 |
| agent 契约文案 | `crates/agent/src/prompt_stack.rs:276` | `canvas_ops_contract()` 指示 agent 只读 ctx、产出 proposal,"Never apply, dismiss, restore, save layout, or mutate graph state directly" | 评估是否需要同步调整(结论:不需要,见设计方案) |
| 前端手动面板 | `web/src/api.ts:297`, `web/src/store.ts`, `web/src/components/manual-proposal-panel.tsx`, `web/src/app.test.tsx` | 面板提交单 op 到 `/proposals/manual`,随后渲染 pendingProposal 等待 apply | 改调新端点,提交即成版;面板交互重构留给 #64/#65/#66 |

## 设计方案

### 1. 新端点形态

`POST /api/workspaces/{workspace_id}/versions/ops`,新文件 `crates/server/src/ops_routes.rs`,在 `main.rs` 注册。

请求体(camelCase 顶层 + `deny_unknown_fields`,op 数组沿用现有 `ManualProposalOpRequest` 线格式并更名 `ManualOpRequest`,新增 `move_node` 变体):

```json
{
  "baseVersionId": "ver_xxx",
  "label": "Manual edit",
  "ops": [
    { "op": "add_node", "id": "n1", "node_type": "input.text", "title": null, "params": { "text": "hi" }, "pos": [0, 0] },
    { "op": "add_edge", "from": ["n1", "text"], "to": ["video", "prompt"], "edge_type": "text" },
    { "op": "set_param", "id": "video", "key": "duration_sec", "value": 5 },
    { "op": "move_node", "id": "video", "pos": [420, 80] }
  ]
}
```

- `label` 可选:缺省时单 op 沿用现有 `manual_title` 风格,多 op 生成 `Manual edit (N ops)`。
- 成功响应:200 + `workspace_state_value`(与 layout 端点一致,前端直接刷新状态)。
- body 解析必须走自定义 rejection 映射:`deny_unknown_fields` 等反序列化失败发生在 axum `Json` extractor 阶段(handler 运行之前),默认的 `JsonRejection` 响应不经过 `ApiError`,不满足结构化 400 契约。新路由使用 `WithRejection<Json<OpsRequest>, ApiError>`(axum-extra)或手动实现 `from_request` 解析 body,把 `JsonRejection` 统一转换为 `ApiError` 的结构化 400 错误体 `{"error": ...}`。

### 2. 原子校验语义(全成全败)

处理顺序,任一步失败即整体拒绝,失败点之前不产生任何持久化写入:

1. 请求形状校验:`ops` 非空、`baseVersionId` 非空、拒绝未知字段(serde `deny_unknown_fields`,经第 1 节的自定义 rejection 映射产出结构化 400)→ 400。
2. 乐观锁:`baseVersionId != workspace.cur_version_id` → 409。
3. pending proposal 守卫:存在 pending proposal → 409(与 layout 端点一致,避免手动编辑使 agent 提案的 base 失效)。
4. 逐 op 转换(迁移自 `manual_op_to_proposal_op`,含 registry 校验、空值/非有限位置检查):第 i 个 op 失败 → 400 + `opIndex: i`。
5. 逐 op 应用并跟踪索引:按数组顺序对内存图逐个应用 op(不整批调用 `GraphService::apply_ops` 后丢失定位),第 i 个 op 应用失败(如引用已被前序 op 删除的节点)→ 400 + `opIndex: i`。
6. 图级整体校验:全部 op 应用完成后 `validate_graph` → 失败 400 + `opIndex: null`(null 仅用于无法归属单个 op 的图级错误,如 CycleDetected、PortTypeMismatch、SetParamConflict)。
7. 持久化(保证 409 路径零持久写):优先先做版本 CAS——`create_version_after(..., source=Manual, parent=base)` 先行(或与图文件登记放在同一事务内),CAS 成功后再写图文件 `workspaces/{workspace_id}/graphs/ops-{uuid_v7}.json`;若实现上 `create_version_after` 必须先拿到图文件(路径/hash),则先写入临时路径(如 `ops-{uuid_v7}.json.tmp`),CAS 成功后原子 rename 到最终路径,CAS 冲突时删除临时文件。
8. 竞态第二道防线:步骤 2 与 7 之间被并发请求抢先时,`create_version_after` 的 CAS 返回 `StoreError::VersionConflict`,ops 路由专门捕获并映射为 409(不走 `ApiError::store` 的 400 映射)。按第 7 步的写入顺序,409 路径不留下任何持久化写入:最终图路径无孤儿文件、无临时文件残留、版本指针不动。

结构化错误体:扩展 `ApiError` 增加 `op_index: Option<Option<usize>>` 语义过重,采用简单方案——`ApiError` 增加可选 `details: Option<serde_json::Value>` 字段,ops 路由在单 op 转换或应用失败时填 `{"opIndex": i}`,图级校验失败时填 `{"opIndex": null}`,`IntoResponse` 时合并进 `{"error": ...}` 体。其他路由不填 details,响应形状不变。

### 3. 与 layout 端点的关系(建议:本 issue 保留不动)

`POST /versions/layout` 保留,理由:

- 它是现有前端拖拽保存的在用消费方,且有专属语义(位置 no-op 过滤:全部位置未变时拒绝成版,避免拖拽抖动刷版本),`/versions/ops` 不做 no-op 过滤(提交什么应用什么)。
- 合并会把前端拖拽改造拖进本 issue,与"只交付 API 与模型统一"冲突。

建议在 #64/#65/#66 画布交互落地时,评估让拖拽保存改走 `/versions/ops`(带 no-op 过滤前置到前端)后删除 layout 端点,记入 tasks.md Handoff Notes。

### 4. CanvasOpsRequest 删除范围

删除(以 `rg CanvasOpsRequest crates web` / `rg "CanvasOp\b" crates web` 无引用为准;限定实现与测试路径,specs/ 下历史规范文档中的文字提及不计):

- `crates/agent/src/canvas_ops.rs`:`CanvasOpsRequest`(:103)、`CanvasOp`(:110)、`CanvasLayoutMove`(:120,仅被 CanvasOp 引用)。
- `crates/agent/src/lib.rs`:对应 re-export(:19-21 中的 `CanvasOp`、`CanvasOpsRequest` 及 `CanvasLayoutMove`)。
- `crates/agent/src/tests.rs`:`rejects_unknown_fields_in_canvas_ops_contract_input`(:197-210)。

保留(已接线,误删会破坏 agent 会话):`CanvasOpsContext`/`CompactCanvasGraph`/`CompactCanvasNode`/`CanvasSelection`/`CanvasGateState`(写入 ctx/canvas_state.json),`CanvasOpsContract`/`CanvasOpSpec`(`lib.rs:100` 写入 ctx/canvas_ops.json)。

服务端删除:`crates/server/src/manual_proposal_routes.rs`、`manual_proposal_routes_tests.rs`、`main.rs` 中 `/proposals/manual` 路由注册。可复用的 `manual_op_to_proposal_op`/`ensure_*`/`manual_title` 辅助函数迁移到 `ops_routes.rs`,不保留旧文件作兼容层。

### 5. prompt_stack 中 CanvasOps 契约文案

不需要调整。`canvas_ops_contract()`(prompt_stack.rs:276)与 `CanvasOpsContract::v1()`(ctx/canvas_ops.json)描述的是 agent 行为契约:agent 只能经 `out/proposal.json` 走 proposal 流程,"Never apply, dismiss, restore, save layout, or mutate graph state directly" 已覆盖"不得调用新端点直接成版"的语义(新端点属于 mutate graph state directly)。agent 路径本 issue 不变,契约文案与契约 JSON 均保持原样;实现时用 agent crate 既有测试确认 ctx 文件内容不变。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 批量成版,无 pending proposal | `ops_routes.rs` happy path | Rust 单测:多 op 请求 → 断言新版本 source=manual、parent=base、`latest_pending_proposal` 为 None、响应含新版本 |
| P2 部分非法整体拒绝 + 结构化错误 | `ops_routes.rs` 转换/应用/校验分支 | Rust 单测:含非法 op 的批量请求 → 400、body 含 `opIndex`、版本指针与图文件未变;单 op 应用失败(`[remove_node n1, set_param n1]`)→ 400 且 `opIndex: 1`;图级错误(成环)→ `opIndex: null` |
| P3 base version 乐观锁 409 | `ops_routes.rs` 守卫 + `create_version_after` CAS 映射 | Rust 单测:陈旧 base → 409;并发模拟(先 advance 版本再提交)→ 409,且断言 409 路径零持久写:graphs 目录无新增文件、无临时文件残留、版本指针不变 |
| P4 pending proposal 守卫 | `ops_routes.rs` 守卫 | Rust 单测:预置 pending proposal → 409 且 proposal 仍 pending |
| P5 undo/restore 兜底 | 既有 `version_routes.rs` | Rust 单测:ops 成版后 undo → source=restore、图回到编辑前 |
| P6 agent 提案路径不变 | `proposal_routes.rs` 及 agent crate 不改动 | 既有 `cargo test --workspace` 全绿;`rg` 确认 proposal apply/dismiss 路由未触碰 |
| P7 空 ops/空 base/未知字段 400 | `ops_routes.rs` serde + 自定义 rejection 映射 + 形状校验 | Rust 单测:空数组、空 baseVersionId → 400;真实 HTTP 层集成测试:POST 带未知字段 → 400 且 body 为结构化 `{"error": ...}`(验证 `JsonRejection` 经自定义 rejection 映射,而非 axum 默认 rejection 响应) |
| P8 agent ctx 文件不变 | `crates/agent` 删除后的会话构建 | 既有 agent 测试(tests.rs:110 断言 canvas_ops.json 存在)保持通过 |
| P9 layout 端点不变 | `layout_routes.rs` 不改动 | 既有 layout 路由测试保持通过 |

## 数据流

- 输入:HTTP JSON(baseVersionId + label + ManualOpRequest 数组)。
- 转换:ManualOpRequest → `ProposalOp`(registry 校验 node_type、补默认 title、SetParam 回填 prev)。
- 校验:逐 op 应用(纯内存,跟踪 `opIndex`)+ `validate_graph`(失败无副作用)。
- 持久化:成功后按设计方案第 2 节第 7 步顺序完成版本 CAS 与图文件写入(`workspaces/{workspace_id}/graphs/ops-{uuid}.json`,含 sha256 hash;`create_version_after` 写 versions 表并 CAS 推进 `workspace.cur_version_id`),CAS 冲突路径零持久写。
- 输出:`workspace_state_value`(workspace/graph/history 全量状态)。
- 外部调用:无(不触达 provider/agent)。

## 备选方案

- 扩展现有 `/proposals/manual` 为"批量 + auto-apply 标志":保留双语义端点,proposal 记录成为无意义中间态,且违背"手动无 pending proposal"的产品决策。放弃。
- 复用 `preview_proposal` 而非逐 op 应用+`validate_graph`:`preview_proposal` 绑定 proposal draft 语义(kind/title/summary/base 检查),手动即时成版不需要 proposal 包装,且整批预览无法定位失败 op 的 `opIndex`;layout 端点先例即直接应用 op。放弃。
- 本 issue 顺带合并 layout 端点:见设计方案第 3 节,推迟到画布交互 issue。放弃。
- 每 op 独立成版(N op → N 版本):撤销粒度更细,但版本历史被批量操作刷屏,且中途失败会留下半套编辑,违背原子语义。放弃。

## 风险

- Security:端点无新增外部输入面,复用既有 registry/图校验;op 数组大小建议沿用 axum 默认 body limit,不新增配置。无密钥/命令执行面。
- Compatibility:破坏性删除 `/proposals/manual`,前后端同 PR 切换;本仓库无外部 API 消费方。历史 proposal 记录只读不受影响。
- Performance:批量 op 在单请求内存中应用,图规模(数十节点)下可忽略;每次成版写一份全量图 JSON,与现状一致。
- Maintenance:新 ops 端点按设计方案第 2 节第 7 步保证 CAS 冲突路径零持久写,不产生无引用图文件;layout 路径既有的 CAS 竞态残留图文件问题不在本 issue 处理,记 Handoff Notes 留待垃圾清理 issue。`ApiError.details` 是通用扩展点,注意不要被其他路由滥用为非结构化数据通道。

## 测试计划

- [ ] Unit tests:`ops_routes.rs` 覆盖 P1-P5、P7 全部分支(happy path、单 op 转换失败 opIndex、单 op 应用失败——`remove_node` 后 `set_param` 同一节点断言 `opIndex: 1`、图级失败 opIndex null、陈旧 base 409、pending proposal 409、CAS 冲突 409 且 graphs 目录零残留、空数组 400、undo 恢复);agent crate 删除后既有测试通过。
- [ ] Integration tests:`cargo test --workspace` 全绿;真实 HTTP 层集成测试:POST 带未知字段 → 断言 400 + 结构化 `{"error": ...}` body(覆盖 axum `Json` extractor rejection 映射);`rg CanvasOpsRequest crates web` 与 `rg "proposals/manual" crates web` 无引用(限定实现路径,排除 specs/)。
- [ ] Manual verification:本地起服务,前端手动面板改参数 → 版本历史立即出现 source=manual 新版本,undo 可恢复;agent 提案仍出现 pending 面板。

## 回滚方案

单 PR 交付、无数据库迁移,直接 `git revert` 该 PR 即回滚:旧 `/proposals/manual` 端点与 `CanvasOpsRequest` 恢复,前端恢复调用旧端点。期间产生的 source=manual 版本与 versions 表既有语义兼容(GH44 先例),无需数据清理。
