# Tech Spec

## Linked Issue

GH-66

## Product Spec

`specs/GH66/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Catalog API | `crates/server/src/registry_routes.rs`, `web/src/types.ts` | `/api/registry/catalog` 返回节点定义、port 和 params schema | 节点库面板的数据源 |
| Canvas selection | `web/src/components/graph-canvas.tsx`, `graph-canvas-selection.ts` | 已有多选、快捷键和 selection clipboard text | 删除/复制/粘贴要复用 selection |
| Proposal ops | `crates/graph/src/lib.rs`, `crates/server/src/manual_proposal_routes.rs` | GraphService 支持 AddNode/RemoveNode/AddEdge/RemoveEdge/SetParam/MoveNode | 添加、删除、粘贴都应生成 proposal ops |
| Store/UI state | `web/src/store.ts` | manual proposal action 返回 workspace state/pending proposal | 新编辑动作应复用同一状态更新 |

## 设计方案

新增 NodeLibrary 侧栏/面板,启动时读取 registry catalog,按 category 分组并支持搜索。添加节点时前端从 NodeDefinition 生成 `GraphNode`:id 使用类型 slug + 短随机后缀,params 根据 params_schema 生成默认值(字符串空串、数字 minimum 或 0、boolean false、enum 第一项),pos 为 drop world point 或 viewport center。提交 AddNode proposal。

Delete 键根据 selection 生成批量 ops:每个选中 node 一个 RemoveNode。由于 GraphService RemoveNode 会删除关联边,不需要额外 remove_edge;若后端批量 ops 不存在,本 issue 与 GH65 共享 batch manual proposal 扩展。复制 payload 使用内部 JSON MIME/clipboard 结构,包含 nodes、edges、sourceVersionId。粘贴时校验 payload,为每个 node 生成新 id,重写内部 edge endpoint,整体偏移,提交 AddNode ops + AddEdge ops 的一个原子 proposal。

所有动作在 pending proposal 存在时禁用。后端仍运行 `preview_proposal` 和 `validate_graph`,失败返回给 UI。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 | NodeLibrary catalog UI | web 测试:分类/搜索/空状态 |
| P2 | add node op generation | web/server 测试:默认 params、唯一 id、drop 位置 |
| P3 | delete selected nodes | graph/server 测试:RemoveNode 删除关联边 |
| P4、P5 | copy/paste payload rewrite | web/graph 测试:新 id、内部边重写、位置偏移 |
| P6 | pending proposal gate | web 测试:pending 时禁用 |

## 数据流

catalog -> NodeLibrary -> add/delete/copy/paste interaction -> batch proposal ops -> server preview -> pending proposal -> apply/dismiss 走既有版本流。

## 备选方案

- 复用系统剪贴板纯文本作为唯一格式:被否,无法安全校验和重写内部边。
- 删除节点时前端手动列出所有关联 remove_edge:被否,GraphService RemoveNode 已负责关联边。

## 风险

- Security: 粘贴 payload 是外部输入,必须 schema 校验并限制字段,不能执行任意内容。
- Compatibility: batch manual proposal 需保持单 op 兼容。
- Performance: 大 catalog 搜索需简单 memoization。
- Maintenance: 默认 params 生成应集中测试,避免各组件重复实现。

## 测试计划

- [ ] Frontend tests: catalog 搜索、add/drop、Delete、copy/paste id rewrite、invalid paste。
- [ ] Server/graph tests: batch AddNode/AddEdge/RemoveNode preview 和 validation failure。
- [ ] Manual verification: 添加节点、删除带边节点、复制粘贴 2 节点子图并 apply。

## 回滚方案

隐藏 NodeLibrary 与快捷键即可回到手动 proposal 表单;batch API 保持兼容或随 PR revert。
