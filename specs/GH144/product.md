# Product Spec

## Linked Issue

GH-144

## 用户问题

存量 workspace 的 legacy graph 缺少 node-embedded semantics。维护者需要在不改写
原版本的前提下，先查看确定性迁移结果并定位 unresolved node，再通过受控入口创建
canonical graph version。GH145 已把 `WorkflowGraph.catalog_revision` 与
`GraphNode.semantics` 定义为唯一 canonical truth；`versions.semantics_json` 只保留为
兼容旧版本的派生索引，不能再作为 v1/v2 判定来源。

## 目标

- 对 workspace current version 提供 connector-bound、secret-free 的 dry-run 报告。
- apply 绑定已确认报告、source graph、catalog、connector、migration version 和稳定
  `operationId`，成功时原子创建一个 `source=migration` 的 canonical child version。
- UI 明确展示 migratable、needs resolution、already migrated、failed、conflict、
  unknown result 与 success；未知结果必须复用同一 `operationId` 查询服务端真相。
- 为 #143 后续清理提供 append-only assessment 与成功 migration audit。

## 非目标

- 不批量迁移历史 version 或跨 workspace 扫描。
- 不删除 #146 的 legacy proposal/read path 或回滚开关。
- 不恢复 GH145 已删除的 layered `WorkflowGraphV2`、capability alias 或旧
  `/graph-migration/*` API。

## Behavior Invariants

1. 同一 source graph、catalog revision、workspace connector 与 migration version 产生
   逐字节稳定报告；`applyEnabled` 不参与 `reportHash`。
2. dry-run 不改变 graph file、version、workspace current、proposal 或 run；每次调用只
   追加 secret-free assessment。
3. source file 缺失、hash/JSON/结构错误使用顶层稳定 code；unknown node type 保留为
   可定位的逐节点 `UNKNOWN_NODE_TYPE`，同时仍校验 schema、endpoint、重复输入与 cycle。
4. node unresolved 使用 typed reason code 和稳定候选顺序；禁止从 title、瞬时 health
   或其他 connector 的 binding 猜测。
5. node-embedded semantics 与 `catalog_revision` 是 canonical truth；合法
   embedded-only graph 返回 `already_migrated`，不得因 sidecar 为空而重复迁移。
6. 旧 sidecar-only version 可作为兼容层验证，但 sidecar 不覆盖 embedded semantics。
7. 成功迁移保持 node/edge ID、title、position、port 与 topology；legacy model 只迁入
   node semantics，不保留重复执行字段。
8. apply 仅接受 server 重算后仍为 migratable 且所有 report precondition 匹配的请求。
9. apply flag `HELIXFLOW_V1_MIGRATION_APPLY` 默认关闭；关闭时 dry-run 仍可用。
10. 同一 `operationId` + 同一 fingerprint 返回同一 target；同 ID 不同输入显式冲突。
11. committed operation replay 发生在 flag/current/report/candidate 检查之前，支持丢
    响应恢复。
12. store transaction 同时 CAS current version、persisted runtime provider 与不存在
    pending proposal；target、current pointer 和 audit 要么全部提交，要么全部回滚。
13. 并发不同 operation 最多一个推进 current；失败 candidate 必须清理或显式报错。
    migration final/temp orphan 必须由启动 reconciliation 识别并回收；workspace 删除不得
    被 migration audit/assessment 的 version 外键阻塞。
14. 原 legacy version 和 graph file 保持不变；回滚沿用 restore。
15. layout、restore、ops、manual proposal 与 agent proposal 保持 embedded semantics，
    并将 `semantics_json` 作为派生索引重建；sidecar-only 历史版本规范化传播，
    不兼容或新 executable node 缺 semantics 时 fail-closed。
16. UI 在 workspace/version/connector 变化时 abort stale dry-run；apply 结果未知时
    保留 operation；晚到成功只在返回原 context 后 hydrate 对应 workspace state；
    无 current version 时不渲染入口，有未提交手工编辑时禁用迁移，预览显示绑定
    connector 与 mapped/structural/unresolved 计数。
17. API、UI、日志与 audit 不包含 credential、auth header、内部 endpoint、raw params
    或 provider 原始响应。

## 验收标准

- [ ] dry-run 确定性、secret-free assessment、source-level failure 与 connector-bound
      mapping 有 Rust/API 测试。
- [ ] embedded-only、sidecar-only、needs_resolution、already_migrated 和成功迁移均有
      明确测试，且 pinned semantics 不被 policy 覆盖。
- [ ] apply flag、report stale、connector stale、current stale、pending proposal、
      同/不同 operation 重放与并发原子性有测试。
- [ ] 迁移后 layout、ops、restore、manual/agent proposal 不丢 embedded semantics。
- [ ] UI 覆盖 server `applyEnabled`、connector/影响计数、逐节点问题、无 current/dirty
      guard、operation ID fallback、explicit confirm、unknown retry、context switch、
      late success hydrate、确定 4xx/503 与 accessibility。
- [ ] Rust workspace、Web type/test/build 与 SpecRail 全量校验通过。

## 发布说明

先以 apply flag 关闭状态发布 dry-run 与 assessment 观测；确认 migratable、
needs_resolution、failed 和 reason code 分布后再灰度 apply。回滚只需关闭 flag；
已创建的 migration version 保持可审计，原 legacy version 可直接 restore。
