# Tech Spec

## Linked Issue

GH-64

## Product Spec

`specs/GH64/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Inspector | `web/src/components/graph-canvas-inspector.tsx` | 参数以只读 summary/span 展示 | 需要改为 schema-driven form |
| Registry schema | `crates/registry/src/lib.rs`, `web/src/types.ts` | catalog 暴露 `params_schema` 的 type/enum/min/max/required | 控件类型和本地校验的事实来源 |
| Manual proposal API | `crates/server/src/manual_proposal_routes.rs`, `web/src/store.ts` | 已支持单个 `SetParam` manual op 并通过 `preview_proposal` 校验 | Inspector 应复用该 API,不新增平行写图接口 |
| Proposal state | `crates/server/src/workbench_payload.rs`, `web/src/types.ts` | pending proposal 会返回 previewGraph 和 diff | 编辑后画布/历史同步走既有机制 |

## 设计方案

前端在 workspace state 中已有 graph node 和 catalog schema。新增 Inspector edit state:选中单节点时根据 node.node_type 找到 NodeDefinition,对每个 params_schema property 渲染控件。提交时构造 `ManualProposalRequest` 的 `SetParam` op,包含 base_version_id、node id、param key、prev/current value 和 typed value。先做本地 parse/validate,再调用现有 manual proposal API。

后端保留现有 `manual_op_to_proposal_op` 和 `GraphService::preview_proposal` 作为最终校验。为改善错误定位,server 可把 `GraphError::SetParamConflict`、registry param validate error 映射为字段级 JSON error,前端若拿不到字段则显示 Inspector 顶部 error。

UI 只允许单节点编辑。存在 pending proposal、review mode 或没有 current version 时禁用提交。提交成功后 store 使用返回的 workspace state 更新 pendingProposal/previewGraph。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1、P2 | Inspector schema controls | web 测试:不同 param type 控件和 enum 下拉 |
| P3、P6 | manual proposal store action | web/server 测试:`set_param` proposal 产生 preview |
| P4 | pending proposal gate | web/server 测试:已有 pending 禁用或 API 返回错误 |
| P5 | local + backend validation | web 测试越界;server 测试 registry 校验错误 |

## 数据流

selection -> NodeDefinition params_schema -> Inspector draft -> local validate -> `POST /api/workspaces/{id}/manual-proposals` set_param -> server preview -> store proposal -> workspace state -> previewGraph 渲染。

## 备选方案

- 前端直接 patch graph state:被否,会绕过 proposal/version gate。
- 为每个参数新增独立后端端点:被否,现有 manual proposal op 已覆盖。

## 风险

- Security: 参数值可能包含路径/URL,沿用 export safety 和 registry validation,不新增 secret 面。
- Compatibility: 前端 schema parser 要兼容 unknown property,不能崩溃。
- Performance: 大参数对象编辑只提交单字段,避免整图 JSON。
- Maintenance: 控件映射集中在 Inspector helper,不要散落到节点组件。

## 测试计划

- [ ] Frontend tests: string/number/integer/boolean/enum 控件、seed random、pending gate、错误展示。
- [ ] Server tests: `SetParam` 类型/范围错误和 stale base conflict。
- [ ] Manual verification: 修改 prompt/seed/尺寸后预览 proposal 并 apply 生成新版本。

## 回滚方案

隐藏 Inspector 编辑控件即可回到只读;后端复用既有 manual proposal,无数据回滚。
