# Manual Graph Proposal Editing Controls

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/46
Locale: zh-CN

## 背景

Helixflow 已支持 Agent 生成 pending proposal，并通过 preview、apply、dismiss 和 `GraphService` validation 保证 graph 变更不会直接绕过安全路径。为了接近成熟 graph workbench，用户需要在 UI 中手动发起受限 graph changes，但这些变更仍必须走同一套 proposal gate。

## 目标

1. 用户可以从 UI 创建手动 pending proposal。
2. 手动 proposal 支持受限操作：从 registry catalog 添加节点、删除节点、连接边、断开边、编辑简单参数。
3. 手动 proposal 必须先显示 preview/diff summary，并复用现有 apply/dismiss 行为。
4. 手动 graph edits 必须由后端 `GraphService` validation 验证。
5. Invalid edge、invalid params、stale base version、已有 pending proposal 必须返回明确错误。

## 非目标

- 不绕过 proposal gate 直接修改 current graph。
- 不执行 run。
- 不引入大型 graph editor library。
- 不实现任意 JSON graph editor。
- 不改变 store schema 或现有 proposal apply/dismiss 语义。

## 用户场景

### 场景 1：从 catalog 添加节点

用户打开手动提议面板，选择 registry catalog 中的 node type，填写 node id 和 params，提交后看到 pending proposal preview。当前 graph 不直接改变。

### 场景 2：删除节点

用户选择现有节点发起 remove node proposal。Preview graph 中该节点及其相关 edges 被移除；用户可以 apply 或 dismiss。

### 场景 3：连接或断开边

用户填写 from/to node port 创建连接，或从现有 edges 中选择断开。无效端口、类型不匹配或重复 input connection 返回明确错误。

### 场景 4：编辑参数

用户选择节点和参数 key，提交 JSON value。后端按 node registry schema 校验参数，不允许 silent fallback。

## 产品需求

| ID | Requirement |
| --- | --- |
| PRD-01 | UI 必须提供手动提议入口，并在已有 pending proposal 时禁用提交。 |
| PRD-02 | UI 必须从 registry catalog 中选择 node type 来创建 add node proposal。 |
| PRD-03 | UI 必须支持 remove node、add edge、remove edge、set param 五类受限操作。 |
| PRD-04 | 手动 proposal 必须先生成 pending proposal，不得直接修改 current version。 |
| PRD-05 | 手动 proposal 必须复用现有 pending proposal preview、diff summary、apply 和 dismiss UI。 |
| PRD-06 | 后端必须使用 `GraphService.preview_proposal()` 校验手动 ops。 |
| PRD-07 | Invalid params、unknown node type、invalid edge、stale base version 和已有 pending proposal 必须以错误返回。 |
| PRD-08 | Apply 后必须创建新 version；dismiss 后 current graph 不变。 |
| PRD-09 | 前端必须显示 manual proposal 创建错误，不得 silent fallback。 |

## 验收标准

- add node、remove node、add edge、remove edge、set param 能生成 pending proposal。
- current `workspace.versionId` 在创建 pending proposal 时不变。
- Pending proposal card 显示 proposal title、summary、diff summary 和 preview graph。
- Apply 后 `workspace.versionId` 变为新 version；dismiss 后 current graph 不变。
- Invalid edge/params 会显示明确错误。
- 测试覆盖 add/remove/connect/disconnect/edit params 的 success/failure。
