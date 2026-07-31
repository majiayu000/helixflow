# Product Spec

## Linked Issue

GH-146

## 已验证事实

- PR #142 于 2026-07-26 合入默认 IntentPlan contract。
- release 1/2：`v0.1.0`，2026-07-26T18:48:29Z，target `3c387d0`。
- release 2/2：`v0.2.0`，2026-07-30T19:33:35Z，target
  `65504bf53f2cabff408518748fd471a709b1c369`。
- GitHub Deployment `5687357563` 将该精确 v0.2.0 build 部署为
  `local-canary`，2026-07-31T06:27:02Z 标记 success。
- v0.2.0 canary 已完成 migrations 1–10、actual-current v1 migration、
  Intent create、legacy rollback modify 和恢复 Intent modify。
- 上述 deployment/rollback smoke 还包含一次预期的负向校验：
  `aspect_ratio=4:3` 不在 catalog 声明的 `1:1 / 9:16 / 16:9` 中，
  compiler 以 `STRUCTURAL_INVALID` fail closed。

这些 smoke 证明 release、迁移、观测和回滚链路可用，但不构成有统计意义的灰度样本集。

## 用户问题

当前生产代码仍允许 `HELIXFLOW_AGENT_INTENT_CONTRACT=0`，Agent 可直接写
`out/proposal.json`，server 和自动修复也保留 legacy proposal 分支。双契约扩大了
prompt、validation、重试、测试和运行时状态空间；但在没有真实灰度证据时删除它，会
失去已经验证的回滚路径。

本 tranche 先冻结可审计的删除 gate，再在 gate 通过后删除 legacy contract。不得用
CI、fixture、空数据库、重复脚本流量或 deployment smoke 冒充自然使用数据。

## 目标

- 为 v0.2.0 IntentPlan 灰度定义可重复查询的时间窗、最小样本和 SLO。
- 用实际 current graph 证明全部存量 workspace 已 canonical migration，或进入另行批准
  的隔离策略。
- 保留一条真实 legacy rollback event，并证明恢复默认 Intent 后可再次成功。
- gate 通过后，让所有 graph-edit Agent turn 只走 `IntentPlan → compiler → proposal`
  路径。
- 删除环境回滚开关、Agent `proposal.json` 输出/read/validation/retry 和 server legacy
  分支。
- 新 head 的替代回滚为部署精确 v0.2.0 release binary，而不是在新 binary 内保留双契约。

## 非目标

- 不删除历史 `agent_contract_observations.contract_mode='legacy'` 记录。
- 不改写 migration 0010，不把历史 legacy observation 伪装成 intent。
- 不删除 server 内部 proposal/version transaction；compiler 仍生成并原子应用 proposal。
- 不删除手工 proposal API、版本历史、undo/restore 或 v1 migration API。
- 不为了通过 SLO 自动制造 graph-edit 请求。
- 不把不受支持的 catalog 参数静默改成默认值。

## 灰度 Exit Gate

### 固定归属

稳定性窗口只接受：

- `releaseId = v0.2.0`
- `buildRevision = 65504bf53f2cabff408518748fd471a709b1c369`
- `since >= 2026-07-31T06:34:00Z`

该起点排除 deployment smoke、legacy drill、预期负向校验和恢复 smoke。若发布新的
build revision，必须为它单独建立窗口；不同 build 不得混在一个通过分母中。

### 时间与样本

同时满足：

1. 窗口跨度至少连续 7 个 24 小时；
2. 至少 20 条 terminal intent observations；
3. observations 覆盖至少 3 个不同 UTC calendar days；
4. 只计人类通过产品发起的真实 CreateWorkflow、ModifyWorkflow、DebugWorkflow；
5. Chat、RunRequest、自动化压测、重复 gate 请求和本 spec 的 smoke 不计入分母。

按日分别查询 evidence API，保存每个非空日的原始 response，防止用单次突发流量冒充
连续灰度。

### Intent SLO

在同一固定窗口：

- `intent.total >= 20`
- `intent.successRate >= 0.90`
- `intent.error / intent.total <= 0.05`
- `intent.clarify / intent.total <= 0.10`
- 以下稳定 error code 必须为 0：
  `PROCESS_INTERRUPTED`、`AGENT_LOG_PERSISTENCE_ERROR`、
  `PROPOSAL_APPLY_ERROR`、`AGENT_OUTPUT_PATH_ERROR`、
  `AGENT_OUTPUT_INVALID`
- 每个非零 clarify/error reason 必须在 gate packet 中逐项解释；不得只报告浮点率。

用户请求超出 catalog 枚举并被 typed error 拒绝是正确 fail-closed 行为，但若发生在正式
窗口内仍计入 error，不得事后排除。只有在窗口开始前明确登记的 smoke 可排除。

### Attribution 与迁移

- `attribution.unattributed = 0`
- `attribution.inFlight = 0`
- `migration.totalCurrentVersions >= 1`
- `migration.complete = true`
- `alreadyMigrated = totalCurrentVersions`
- `migratable = needsResolution = failed = missingOrCurrentless = 0`

当 `migration.complete=true` 且没有 unresolved workspace 时，
`APPROVED_ISOLATION_UNSUPPORTED` 只说明系统未实现隔离审批模型，不阻塞本 gate，因为本
窗口没有需要隔离的数据。任何 unresolved 数据出现时必须停止并另行批准隔离方案。

### Rollback

删除前必须保留以下真实证据：

1. 同一 v0.2.0 attributed database 至少一条
   `legacy/success/LEGACY_PROPOSAL_APPLIED`；
2. legacy drill 前后 health/ready green；
3. 恢复默认 Intent contract 后至少一条新的 intent success；
4. actual-current migration 在 drill 后仍 complete。

2026-07-31 canary 已满足该删除前 drill。正式 gate packet必须引用 Deployment
`5687357563` 和对应 observation counts。

## Gate Packet

维护者在开始删除实现前，把下列事实作为一条不可编辑的 issue comment发布：

- release/tag/commit 与 GitHub Deployment ID；
- window `since` / `until`、跨度和覆盖的 UTC days；
- exact evidence API query；
- 每日 raw responses 和总窗口 raw response；
- intent/legacy 原始整数、rate 与 reason maps；
- attribution、migration 和 limitations；
- rollback drill 时间、恢复 Intent success 和 health/ready；
- 明确 `passed` 判定及逐条阈值计算。

API 本身继续不返回 `passed`；判定只存在于维护者 gate packet。

## 删除后的产品行为

1. graph-edit turn 总是要求 `out/intent.json`。
2. Agent 输出 `proposal.json`、低层 graph ops 或未知文件时显式失败。
3. server 总是编译 IntentPlan；不存在 env flag、fallback 或 warning-only legacy path。
4. 自动修复也只走 IntentPlan/compiler。
5. 旧 graph 必须先通过 migration API成为 canonical current；否则显式返回迁移错误。
6. 历史 legacy observations 继续可查询，但不会再产生新的 legacy observation。

## 替代回滚

删除实现出现 release-blocking regression 时：

1. 停止删除版 binary，不改写数据库；
2. 在 canary 数据库副本上启动精确 `v0.2.0@65504bf...` binary；
3. 不设置 `HELIXFLOW_AGENT_INTENT_CONTRACT=0`，先验证默认 Intent read/write；
4. 验证 health/ready、current graph、版本历史和 evidence API；
5. 只有副本演练成功才允许把生产部署回滚到 v0.2.0；
6. 修复 forward head，禁止在删除版重新加入 silent fallback。

migration 0010 是 additive，v0.2.0 可以忽略删除版没有新增的应用代码；若删除实现需要
新的 destructive schema migration，则本 spec 失效，必须重新审批。

## 验收标准

- [ ] 固定灰度窗口满足全部时间、样本、SLO、attribution 和 migration gate。
- [ ] issue 上存在可复核的 gate packet，不使用 smoke/fixture/合成流量。
- [ ] 删除 `HELIXFLOW_AGENT_INTENT_CONTRACT` 与 `use_intent_contract`。
- [ ] Agent graph-edit contract 不再创建、读取、校验或重试 `out/proposal.json`。
- [ ] server normal graph edit 与 run-fix 不再调用 legacy `propose_graph_change`。
- [ ] 新 graph-edit observations 只能写 `contract_mode=intent`。
- [ ] 历史 legacy observations 仍可读，且数据库 migration 不丢失历史。
- [ ] unsupported catalog value 继续 typed fail closed，不新增 alias/default fallback。
- [ ] 删除后在 canary DB 副本完成 v0.2.0 binary rollback 演练。
- [ ] Rust/Web 全量验证、exact-head review 和 CI green。

