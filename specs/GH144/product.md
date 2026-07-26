# Product Spec

## Linked Issue

GH-144

## 用户问题

HelixFlow 已具备 graph v1→v2 的确定性 migrator，但维护者和 workspace 用户没有
可见、可控的迁移入口。旧 workspace 只能在运行时继续依赖兼容读取，用户无法提前
知道哪些节点可自动迁移、哪些节点需要选择模型，也无法获得后续删除 legacy path
所需的迁移证据。

## 目标

- 允许用户对 workspace 当前 version 先执行零写入的 dry-run，再决定是否 apply。
- 用稳定、逐节点的报告说明可迁移、需要解决、已迁移和失败状态。
- 迁移成功时创建新的 v2 version，保留原 v1 version 作为不可变历史。
- 对并发、过期报告、重复提交和部分失败采取显式、可重试且 fail-closed 的行为。
- 为 #145 的 capability/schema 收敛和 #146 的 legacy 删除 gate 提供可审计证据。

## 非目标

- 不删除 `canonical_capability`、legacy proposal 契约或
  `HELIXFLOW_AGENT_INTENT_CONTRACT`。
- 不把 `semantics/implementation` 嵌入 `GraphNode`，不执行 #145 的 schema 收敛。
- 不批量改写历史 version，不覆盖或删除原 v1 graph。
- 不从 node title、相似字符串或 provider 默认值推断 model。
- 不引入新的用户、角色或 workspace 权限模型。

## Behavior Invariants

1. 用户从明确的 workspace 当前 version 发起迁移；报告始终显示
   `workspaceId`、`sourceVersionId`、source graph identity、`migrationVersion`
   与 `catalogRevision`，不会隐式迁移其他 workspace 或 version。
2. dry-run 对 durable graph、version history、workspace current version、proposal
   和 run 均为零写入；刷新页面后原 workspace 状态保持不变。
3. 在 source graph、catalog revision 与 migration version 相同的前提下，重复
   dry-run 产生相同的迁移判定、node 顺序和 reason code。
4. 报告顶层状态只能是 `migratable`、`needs_resolution`、
   `already_migrated` 或 `failed`；每个 source node 恰好对应一条有稳定 action
   或 reason code 的结果。
5. graph 中未声明 model 时只能采用已配置且唯一的 policy default binding；
   model 缺失、歧义、不存在或 capability 不匹配时显示 `needs_resolution`，禁止
   猜测或 silent fallback。
6. dry-run preview 与成功迁移后的 graph 保持原 node/edge ID、title、position、
   port connection 和 topology；仅删除已迁入语义层的 legacy execution 字段。
7. `needs_resolution` 报告必须定位到 node，并提供安全、可理解的原因和可选候选项；
   任一 executable node 未解决时，整个 version 的 apply 被阻止。
8. apply 必须来自用户对一份成功 dry-run 的显式确认，并绑定该报告的 source graph、
   catalog revision 与 migration version；不能提交任意客户端构造的迁移结果。
9. 若 dry-run 后 workspace current version、source graph、catalog revision 或
   migration version 已变化，apply 返回 conflict 并要求重新 dry-run，不自动使用
   新数据重算后继续。
10. 单次 apply 以一个 version 为原子边界：成功时创建一个新的 v2 version 并原子
    推进 workspace current version；失败时 graph 文件、version record 与 current
    pointer 均不留下半迁移状态。
11. 原 v1 version 及其 graph 保持不可变。新 v2 version 可通过既有 version history
    识别其迁移来源，用户仍可按既有恢复行为回到旧 version。
12. apply 接受稳定 `operationId`。相同 `operationId` 与相同输入的重试返回同一
    结果；相同 `operationId` 携带不同输入时显式冲突；两个并发 apply 最多产生一个
    新 current version。
13. 对已经是完整 v2 的当前 version，dry-run 返回 `already_migrated`，apply 不创建
    新 version。
14. UI 明确区分 loading、migratable、needs resolution、applying、success、
    conflict 和 failure；请求失败或页面刷新后必须重新读取 server truth，不把未知
    状态显示成成功。
15. 迁移入口沿用现有 workspace 读取和变更边界；无权读取或修改目标 workspace 的
    请求不得通过迁移入口扩大权限。
16. API、UI、日志和迁移报告不包含 credential、内部 endpoint、auth header 或
    provider 原始响应；未知内部错误显示稳定 code 与安全摘要。
17. 成功 apply 持久化 source/target version identity、graph identity、
    `migrationVersion`、`catalogRevision`、`operationId`、结果摘要和时间，使维护者
    能统计已迁移、待解决及失败数量。

## 验收标准

- [ ] 对同一 v1 version 连续 dry-run，响应逐字节稳定且数据库、graph 文件及
      workspace current version 均无变化。
- [ ] 可解析的 v1 version apply 后产生一个新的 v2 current version，原 node/edge
      topology 与原 v1 version 均保持不变。
- [ ] model 歧义、unknown node、缺少 default binding 和 capability mismatch 均以
      稳定 reason code 阻止 apply。
- [ ] source version、graph hash 或 catalog revision 在确认前变化时返回 conflict，
      且不会创建 version 或遗留 graph 文件。
- [ ] 相同 `operationId` 重试幂等；并发不同 operation 最多一个成功推进 current
      version。
- [ ] UI 能定位每个待解决 node，完整呈现 loading/error/conflict/success，并可在
      刷新后恢复 server truth。
- [ ] 成功迁移记录足以按 workspace/version 汇总迁移覆盖率，且所有外部响应通过
      secret-free 回归检查。

## 边界情况

- 空 graph 可以迁移，报告为空 node 列表并保持空 topology。
- source version 不属于 path 中的 workspace、version 不存在或 workspace 无 current
  version 时显式拒绝。
- source graph 文件缺失、hash 不匹配或 JSON 无法解析时返回 `failed`，禁止修补后
  继续。
- catalog 中 connector 暂时 unavailable 不改变 migration mapping；catalog/binding
  定义缺失或歧义仍阻止迁移。
- 用户在 dry-run 后编辑画布、apply proposal、undo/restore 或切换 current version
  时，旧报告立即过期。
- 网络中断后客户端不得猜测 apply 是否成功，必须按 `operationId` 重新查询/重试。

## 发布说明

- 首先只发布 dry-run 和报告 UI，验证报告稳定性与 secret-free 输出；apply 保持关闭。
- apply 灰度开启后，持续统计 `migratable`、`needs_resolution`、conflict、failed
  与成功迁移数量。
- 回滚只关闭新的迁移入口，不覆盖或删除已成功创建的 v2 version。
- #145 只能在迁移入口和证据满足其 gate 后删除运行期 capability mapping；
  #146 仍需等待两个明确 release 与稳定性证据。
