# Task Plan

## Linked Issue

GH-59

## Spec Packet

- Product: `specs/GH59/product.md`
- Tech: `specs/GH59/tech.md`

## 实现任务

- [ ] `SP59-T1` Owner: backend-server. 新增 `crates/server/src/ops_routes.rs` 与 `POST /api/workspaces/{workspace_id}/versions/ops`:批量 `ManualOpRequest`(含 move_node)转 `ProposalOp`、原子校验(全成全败,逐 op 应用并跟踪 `opIndex`)、直接成版 source=manual;body 解析用自定义 rejection 映射(`WithRejection<Json<T>, ApiError>` 或手动 `from_request`)把 `JsonRejection` 转为结构化 400;扩展 `api_error.rs` 可选 `details` 输出 `opIndex`;图文件先 temp 写入并原子 rename 到最终路径,commit gate 在同一事务/锁内同时检查 base version 与 pending proposal,冲突/DB 失败清理 final/tmp 文件并映射 409,成功版本不得指向未写好的图文件。Done when: tech.md 设计方案第 1/2 节全部行为落地且 P1-P5、P7 对应单测与 HTTP 层未知字段集成测试通过。Verify: `cargo test -p helixflow-server`
- [ ] `SP59-T2` Owner: backend-server. 删除 `crates/server/src/manual_proposal_routes.rs`、`manual_proposal_routes_tests.rs` 与 `main.rs` 中 `/proposals/manual` 注册,可复用辅助函数(`manual_op_to_proposal_op`/`ensure_*`/`manual_title`)迁入 `ops_routes.rs`,不留兼容层。Done when: `rg "proposals/manual" crates web` 无引用(限定实现路径,specs/ 不计)且 workspace 编译通过。Verify: `cargo check --workspace`
- [ ] `SP59-T3` Owner: backend-agent. 删除 `crates/agent/src/canvas_ops.rs` 中 `CanvasOpsRequest`/`CanvasOp`/`CanvasLayoutMove`、`lib.rs` 对应 re-export、`tests.rs` 的 `rejects_unknown_fields_in_canvas_ops_contract_input`;保留 `CanvasOpsContext` 系列与 `CanvasOpsContract`/`CanvasOpSpec`(已接线到 ctx 文件);`prompt_stack.rs` 契约文案不动。Done when: `rg CanvasOpsRequest crates web`、`rg "CanvasOp\\b" crates web`、`rg CanvasLayoutMove crates web` 均零命中(限定实现路径,specs/ 历史规范文档不计)且 agent 既有测试(含 ctx/canvas_ops.json 存在性断言)通过。Verify: `cargo test -p helixflow-agent`
- [ ] `SP59-T4` Owner: frontend. `web/src/api.ts`/`store.ts`/`types.ts` 将手动编辑请求切换到 `POST /versions/ops`(批量 op 数组、baseVersionId 乐观锁),`manual-proposal-panel.tsx` 与 `manual-proposal-helpers.ts` 提交后直接刷新工作区状态(无 pendingProposal 等待),`app.test.tsx` 同步更新;面板交互重构不做(留 #64/#65/#66)。Done when: 手动编辑提交即出现新版本、409/400 错误(含 opIndex)可提示,web 测试通过。Verify: `cd web && npm test -- app.test.tsx`

## 并行拆分

三条实现 lane 文件所有权互不重叠,可并行;lane 内串行:

- backend-server lane(SP59-T1 → SP59-T2):独占 `crates/server/src/ops_routes.rs`(新建)、`crates/server/src/main.rs`、`crates/server/src/api_error.rs`、`crates/server/src/manual_proposal_routes.rs`(删)、`crates/server/src/manual_proposal_routes_tests.rs`(删)。
- backend-agent lane(SP59-T3):独占 `crates/agent/src/canvas_ops.rs`、`crates/agent/src/lib.rs`、`crates/agent/src/tests.rs`。
- frontend lane(SP59-T4):独占 `web/src/api.ts`、`web/src/store.ts`、`web/src/types.ts`、`web/src/components/manual-proposal-panel.tsx`、`web/src/components/manual-proposal-helpers.ts`、`web/src/app.test.tsx`。按 tech.md 第 1 节的线格式开发,无需等待 SP59-T1 完成,联调在 SP59-T5。
- 任何 lane 不得触碰 `crates/graph`、`crates/store`、`crates/server/src/layout_routes.rs`、`crates/server/src/proposal_routes.rs`、`crates/agent/src/prompt_stack.rs`。
- SP59-T5 由 coordinator 在三条 lane 合并后串行执行。

## 验证

- [ ] `SP59-T5` Owner: coordinator. 全量回归与验收对照:逐项核对 product.md 验收标准与 Behavior Invariants P1-P9。Done when: 以下命令在本会话全部通过且 `rg CanvasOpsRequest crates web`、`rg "CanvasOp\\b" crates web`、`rg CanvasLayoutMove crates web` 与 `rg "proposals/manual" crates web` 零命中(限定实现路径,specs/ 历史文档中的提及不计)。Verify: `cargo fmt --check && cargo check --workspace && cargo test --workspace && (cd web && npm test && npm run build) && python3 checks/check_workflow.py --repo . --spec-dir specs/GH59`

## Handoff Notes

- 产品决策已由 owner 拍板,实现时不得改动:手动直接编辑跳过二次确认即时成版(source=manual,undo/restore 兜底);agent 提案保持 pending → apply/dismiss 不变。
- `CanvasOpsContract`/`CanvasOpSpec` 不是死代码(`crates/agent/src/lib.rs:100` 写入 ctx/canvas_ops.json),删除范围严格限于 `CanvasOpsRequest`/`CanvasOp`/`CanvasLayoutMove`,详见 tech.md 设计方案第 4 节。
- `POST /versions/layout` 本 issue 保留不动;#64/#65/#66 画布交互落地后评估将拖拽保存迁到 `/versions/ops` 并删除 layout 端点。
- 已知遗留(不在本 issue 处理):layout 路径 `create_version_after` CAS 竞态窗口内可能残留无引用图文件,留待后续垃圾清理 issue;新 `/versions/ops` 端点按 tech.md 设计方案第 2 节第 7 步保证成功版本只指向已写好的最终图文件,且冲突路径清理 final/tmp 文件,不引入同类残留。
- 结构化错误约定:400 体 `{"error": string, "opIndex": number|null}`,单 op 转换或应用失败给该 op 序号(含 remove_node 后 set_param 同一节点这类序内冲突,应用到该 op 时失败给其索引),全部应用完成后的图级校验失败才给 null;前端据此高亮失败 op。
