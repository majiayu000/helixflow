# Tech Spec

## Linked Issue

GH-153

## Product Spec

见 `specs/GH153/product.md`。

## Current Codebase Context

| Area | Current files | Existing contract / required change |
| --- | --- | --- |
| Retry finalization | `crates/run/src/self_heal.rs`, `sweep_background.rs` | 当前 `prepare_retry` 用 `Option` 合并 success、pending、cap exhausted 等停止原因；改为 typed decision，并只在明确 exhausted 时交给 terminal finalizer |
| Compile and cost gate | `crates/run/src/cost_gate.rs`, `run_policy.rs`, `retry_records.rs` | retry 复制旧 plan/estimate；fix child 必须走 fresh compile/resolve/estimate，复用 confirmation threshold，但不复用 `runs.attempt` |
| Agent debug | `crates/agent/src/turn_mode.rs`, `service.rs`, `prompt_stack.rs` | 复用 `DebugWorkflow` / `FixError` 与当前 IntentPlan 开关；后台调用绕过关键词分类 |
| Server orchestration | `crates/server/src/app_state.rs`, `workbench_message.rs`, `workbench_message_intent.rs`, `workbench_message_proposals.rs` | 抽取 exact-run 脱敏 context 与 proposal auto-apply primitives；新增后台 fix coordinator 和启动恢复 |
| Persistence | `crates/store/migrations/0001_initial.sql`, `0004_run_retry_and_artifact_review.sql`, `run_records.rs`, `proposal_records.rs` | 新增独立 fix attempt/audit 状态与原子 CAS/linkage，不改变 `runs.attempt` |
| Version files | `crates/server/src/version_file_consistency.rs`, `version_file_reconciliation.rs` | 复用 candidate publish/commit/cleanup；修复图不得绕过 graph hash 与启动 reconciliation |
| Events / UI | `crates/store/src/run_records.rs`, `web/src/store-events.ts`, `types.ts`, `components/run-panels.tsx` | run event 先持久化后广播；新增 fix notice，继续复用现有 run snapshot 与 confirmation UI |

## Architecture

### 1. Typed terminal failure handoff

`crates/run` 不依赖 `crates/agent`。在 run 层引入小型 terminal failure contract（建议
独立 `failure_finalizer.rs`）：

- `RetryDecision::Continue(prepared)`：同图 retry 已 claim，继续执行；
- `RetryDecision::Pending(child)`：retry child 等待 cost confirmation，不触发 fix；
- `RetryDecision::Exhausted(terminal)`：run 为 failed 且同图额度明确耗尽；
- `RetryDecision::Stop(reason)`：success/interrupted/ineligible/config 或基础设施异常。

`run_with_self_heal` 返回最终 run/decision，而不是把所有 `None` 当作相同终态。但
correctness 不依赖该返回值恰好被同一进程消费：feature 开启时，eligible run 的 failed
terminal transaction 同时 insert-or-read `run_failure_work_items`。work item 至少携带
`source_run_id`、durable repair chain/provenance 与稳定 reason，不携带 raw error。
coordinator claim 后在一个 transaction 中创建 retry child 或把 item 推进为
`fix_pending`；启动恢复扫描未完成 item。

普通 agent 在首个 eligible terminal 前建立/关联 chain。feature 开启时，
`start_confirmed_sweep` 必须把 `group_id + recommended_run_id + chain` 与 sweep
confirmation/全部成员 claim 放在同一 store transaction，并在任何成员进入
`queued`/`running`、启动 background task 或远端 dispatch 之前提交。失败 terminal
transaction 只引用这个已存在的 chain；不得等到推荐 run 已失败、准备调用 self-heal 时
再补写。retry child 显式继承 chain id，不能从会变化的 `trigger`/`group_id` 反推。
非推荐成员没有 chain，因此即使同一 crash window 被扫描也不能触发。该 durable handoff
同时是 GH-154 recovered-failed 的唯一入口。

### 2. Configuration

在 run policy 或 server fix policy 中增加严格 parser：

- `HELIXFLOW_RUN_AGENT_FIX_ENABLED`：缺失/`0`/`false`/`off` 为 false，
  `1`/`true`/`on` 为 true；其他值显式配置错误。
- `HELIXFLOW_RUN_MAX_FIX_ATTEMPTS`：只在开关开启后解析；缺失为 `1`，非负 `u32`；
  非法值 fail-closed。开关关闭时忽略该值，保持零写入。

开关检查发生在 durable attempt claim 之前，因此关闭态没有写放大。max 为 0 或非法配置
时写一次幂等 exhausted event；不得调用 Agent。

### 3. Durable chain、work item、attempt 与 event outbox

下一条 store migration（实现时以 main 的下一个空闲序号命名，当前基线候选为
`0008_run_agent_fix.sql`）新增：

- `run_repair_chains`：`root_run_id` UNIQUE，保存 `provenance`、可空
  `sweep_group_id`/`recommended_run_id`、配置快照与创建时间；
- `run_failure_work_items`：`source_run_id` UNIQUE，保存 chain、typed retry decision、
  `pending/claimed/retry_created/fix_pending/completed` 状态和 claim lease；
- `run_fix_attempts`：独立 Agent 调用状态；
- `run_fix_event_outbox`：`dedupe_key` UNIQUE，保存安全 event payload 与投递状态。

`run_fix_attempts` 建议字段与约束：

| Field | Contract |
| --- | --- |
| `id`, `operation_id` | 稳定 ID；`operation_id` UNIQUE，用于重放 |
| `workspace_id` | FK workspace，删除 workspace 时级联 |
| `root_run_id`, `source_run_id` | repair chain 根与本次精确 failed run；FK run |
| `attempt_index`, `max_attempts` | 1-based 独立 fix 计数；UNIQUE(root_run_id, attempt_index) |
| `source_version_id` | Agent 输入和 version CAS 快照 |
| `expected_runtime_provider_id` | workspace nullable selection 的 CAS 快照 |
| `effective_provider_id` | 实际 compile/estimate/run provider，非空 |
| `expected_recovery_scope_fingerprint` | 复用 GH-154 的 account/tenant/credential identity scope，非空且不含 raw secret |
| `provider_catalog_fingerprint` | Agent 可见 provider/model catalog 的非秘密 revision/fingerprint |
| `state` | `claimed`, `agent_running`, `version_applied`, `child_preparing`, `child_ready`, `failed`, `exhausted` |
| `proposal_id`, `target_version_id`, `child_run_id` | 可空 FK；阶段推进后填充 |
| `reason_code`, `error_summary` | 稳定 code 与脱敏、截断摘要；禁止 raw payload |
| timestamps | created/updated/finished，用于恢复审计 |

store API 使用 `BEGIN IMMEDIATE` 和条件 UPDATE 实现：

1. 只从显式 chain/work-item/child linkage 解析 root provenance，统计已消耗 attempt；
2. 同一 source 的 active operation 或同一 `(root_run_id, attempt_index)` 只允许一个；
3. 达到上限幂等返回 exhausted；
4. 状态只按上述顺序前进，终态不可回退；
5. `version_applied` 及以后 replay 返回已有 target，不重新调用 Agent；
6. child 创建与 `child_run_id` linkage 在单 transaction 内推进到 `child_preparing`，
   operation replay 返回同一 child；
7. Agent failure/clarify/invalid 将 attempt 标为 failed；若 chain 尚有额度，coordinator
   立即 claim 下一 index，达到上限才写 exhausted；
8. attempt/decision 变化与 outbox `pending` insert 同事务；dispatcher 在 transaction
   中用确定性 event id insert-or-read `run_events`，把 outbox 推进为
   `event_persisted` 后 commit，再广播；只有广播成功后才标 `broadcasted`。进程在 event
   commit 后、broadcast 前退出，或广播成功后、标记前退出，启动扫描都可安全重播，客户端
   按 event id 去重。max=0/非法配置也在 chain 上有唯一 decision/dedupe key，不需要伪造
   1-based attempt。

attempt 表不保存 graph、raw `error_json`、prompt 或 provider response。workspace 删除
不得被 attempt 的 run/version/proposal 外键阻塞。

### 4. Exact-source DebugWorkflow request

server 新增 `run_agent_fix.rs` coordinator，并将 `workbench_message.rs` 当前私有的
`safe_error_summary` / exact failed-step formatter 抽成可复用 helper。coordinator 按
`source_run_id` 查询：

- source run 必须仍为 `failed`；
- source version 必须属于同一 workspace，graph file/hash 必须验证；
- workspace current version、nullable runtime provider selection、effective provider、
  GH-154 recovery scope 与 provider catalog fingerprint 必须等于 attempt 快照；
- run provenance 必须来自 durable chain；调用参数、trigger 和 event 不具有认证作用。

构造 `AgentSessionRequest` 时固定：

- `mode = TurnMode::DebugWorkflow`；
- `skill = AgentSkill::FixError`；
- `base_version_id` / safe graph projection 来自 source version；
- `run_context` 只含 source run 和 failed steps 的脱敏摘要，并置于明确的 untrusted-data
  delimiter 中；
- `history = []`，`user_message` 为 server-owned、无 raw error 的修图指令；
- provider catalog 来自已快照并再次验证的 workspace provider；
- `use_intent_contract` 沿用 `AppState` 当前配置。

低级 proposal 与 IntentPlan 两条路径都必须产出 `ProposalKind::Fix`，再经过现有
validation/compiler。固定 system policy 明确 graph/error/params 都是数据而非指令；
projection 删除 secret-bearing、超长和与结构无关的自由文本，只保留修图所需节点类型、
端口、拓扑和截断参数。server 从 failed step 与 frozen plan 确定允许修改的节点/依赖闭包，
proposal diff gate 禁止修改闭包外节点/边、删除无关节点或改变 workspace/provider 设置。
无法在闭包内完成时 fail closed 转人工。clarify 不是成功修复；后台没有用户可回答的同步
turn，因此消耗 attempt，并在有剩余额度时立即进入下一 attempt。

Agent session 是本地运行过程证据，不是 provider run，不写 `cost_ledger`。日志与消息
仍按既有安全边界持久化，但不得包含未经脱敏的 source error。

### 5. Atomic fix version apply

将 `workbench_message_proposals.rs` 的 candidate 构造/校验复用为内部 primitive，并为
自动修图提供专用 store transaction。提交必须同时验证：

- attempt 仍在可提交 state，operation/fingerprint 未改变；
- source run 仍为 `failed`；
- workspace `cur_version_id = source_version_id`；
- workspace `runtime_provider_id IS expected_runtime_provider_id`；
- registry 当前解析的 `effective_provider_id`、GH-154
  `recovery_scope_fingerprint` 与 `provider_catalog_fingerprint` 均等于快照；
- proposal base/target graph hash 与 candidate 一致。

transaction 原子插入 applied fix proposal、`source=proposal` child version、审计 message，
推进 current，并把 attempt 标为 `version_applied`、写 target/proposal ID。外部 graph
candidate 先 publish，DB 成功后 mark committed；DB/CAS 失败走引用感知 cleanup。

不得调用现有只检查 current version 的 auto-apply 后再单独 UPDATE attempt，因为重启窗口
会造成“version 已提交但 operation 不知情”。provider 变化与 version 变化均返回稳定
conflict，source run 保持 failed。

### 6. Fresh child run and cost gate

从已提交 target version 重新读取 graph，使用 attempt 的 `effective_provider_id` 调用
`GraphService::compile_plan`、semantic resolution 与 provider estimate。新增幂等
fix-child prepare primitive，语义等价于 `prepare_agent_run`，但必须：

- `trigger = agent`；
- `runs.attempt = 0`，不复制 source retry attempt；
- 不复制 source `plan_json`、`estimate_json`、force-rerun 或 ledger；
- 新 run/steps 与 attempt `child_run_id` 在单 transaction 内唯一关联，并把 attempt 推进
  为 `child_preparing`；
- 同一 operation replay 返回相同 child。

每个 step 估价使用 GH-154 的 `estimate:{run_step_id}` 唯一 operation key 和
insert-or-read payload 校验；compile 可确定性重放，已写 estimate 不重复。进程在任意
step estimate 后退出时，fix recovery 识别 `child_preparing`/`estimating` child，复用同一
run/step 与 ledger，补齐缺失项；GH-154 的通用 estimating 清理必须排除有 active
fix-attempt linkage 的 child。全部 estimate 完成后，单 transaction 写 aggregate
estimate/cost decision、推进 attempt `child_ready`，再把 child 置为
`waiting_confirmation` 或可 claim。

`expected_recovery_scope_fingerprint` 不只在 version apply 时检查。每次 step estimate、
threshold 内 auto-start、等待确认后的用户 confirm、restart recovery 与 provider dispatch
都必须从 registry 重新解析 effective provider/scope/catalog 并与 attempt 快照相等。
scope 改变时把 child fail closed 为 `FIX_PROVIDER_SCOPE_CHANGED`（target version 保留用于
审计），不得调用 estimate 或 provider；通用 `start_confirmed_run` 对带 fix-attempt
linkage 的 child 必须执行同一 guard，不能旁路。

child 估价完成后调用既有
`start_confirmed_run_within_budget`：未知/超阈值保持 pending；阈值内按既有 workspace
atomic claim 自动执行。只有 child 可查询且 cost decision 已持久化后，才在 source run
写 `run.fix_applied`。

child 再次失败时先按其 `runs.attempt` 做同图 retry；typed finalizer 从
`run_fix_attempts.child_run_id` 解析原 root chain，只有 retry exhausted 后才 claim 下一个
fix attempt。

### 7. Restart recovery and idempotency

`AppState::open_in_data_dir` 在 store migration、version-file reconciliation 之后启动 fix
recovery。恢复规则：

- `claimed` / `agent_running`：进程内 Agent handle 已丢失；将该 attempt 记为已消耗
  `failed`，不得以同一 operation 重放 Agent。若仍有额度，立即 claim 新 attempt；
  否则写 exhausted。
- `version_applied`：读取并验证已记录 target，幂等创建/恢复同一 child；不得再次生成
  proposal/version。
- `child_preparing`：复用同一 child/steps 和 stable estimate keys 补齐估价，禁止创建
  新 run 或重复 ledger。
- `child_ready` / `failed` / `exhausted`：只消费 durable outbox 或继续观察 child，
  不创建副本；failed 且仍有额度时按正常 coordinator 语义推进下一 attempt。
- 未完成 `run_failure_work_items`：按 durable chain/provenance 恢复 retry/fix decision；
  不从 trigger、group 或最新 run 猜测来源。

event bus 不是恢复真相；所有决策来自 DB/outbox。`pending → event_persisted` 与 run event
写入同事务，broadcast 成功后才推进 `broadcasted`；任何中间 crash 都以相同 event id
at-least-once 重播，snapshot/refetch 与 UI 按 id 去重。

GH-154 先恢复/收敛持久化 remote handles，再把确实收敛为 `failed` 且符合来源条件的
run 交给同一 durable work-item finalizer。仍在 remote polling、补取消或被标为
`interrupted` 的 run 不进入 fix。GH-153 不创建或模拟任何 provider remote handle。

### 8. Event and frontend contract

三个 persisted event 的安全 payload：

| Event | Required data |
| --- | --- |
| `run.fix_attempt` | `operation_id`, `attempt`, `max_attempts`, `source_version_id` |
| `run.fix_applied` | 上述字段 + `target_version_id`, `child_run_id`, `requires_confirmation` |
| `run.fix_exhausted` | `attempts`, `max_attempts`, stable `reason_code`, safe `error`（可选） |

event 挂在 exact source run；outbox dedupe key 至少包含 chain/source、event kind 与
attempt index（无 attempt 时用 decision kind），不得包含 graph、params、raw error 或
provider endpoint。Web
`applyRunEvent` 在 retry 分支之前识别 fix events，生成 `run-fix-*` system notice；
`preserveRetryNotices` 扩展为同时保留 fix notices。`shouldRefetchWorkspaceState` 对
`fix_applied` 和 `fix_exhausted` refetch。现有 pending confirmation payload 与 modal
不新增旁路。

## Failure Codes

至少提供稳定 code：

- `FIX_DISABLED`
- `FIX_LIMIT_REACHED`
- `FIX_INVALID_CONFIGURATION`
- `FIX_SOURCE_INELIGIBLE`
- `FIX_SOURCE_CHANGED`
- `FIX_PROVIDER_CHANGED`
- `FIX_PROVIDER_SCOPE_CHANGED`
- `FIX_AGENT_FAILED`
- `AGENT_CLARIFICATION_REQUIRED`
- `FIX_PROPOSAL_INVALID`
- `FIX_VERSION_CONFLICT`
- `FIX_CHILD_PREPARE_FAILED`
- `FIX_RECOVERY_INTERRUPTED`

用户消息只显示脱敏摘要；完整 raw provider/Agent payload 不进入 attempt 或 run event。

## Product-to-Test Mapping

| Invariants | Test focus |
| --- | --- |
| 1–6 | policy parser、关闭态无写入、durable work item、agent/recommended provenance |
| 7, 16–19, 22 | store claim、chain 计数、N>1 推进、outbox/event 幂等 |
| 8–10 | exact source、安全 projection、prompt injection 与 scope/diff gate |
| 11–12 | nullable/effective provider CAS、proposal/version/attempt 原子性、candidate cleanup |
| 13–15 | target graph fresh compile、partial estimate recovery、cost gate/ledger 幂等 |
| 18–20 | restart matrix、single child、child_preparing、exactly-once event |
| 21 | Web retry/fix notices、snapshot preservation、confirmation/refetch |
| 23 | GH-154 common finalizer/work-item contract 的 unit/integration seam |

## 风险与回滚

- Security：exact-run redaction + untrusted-data system policy + safe graph projection +
  deterministic scope/diff gate；事件/attempt 不存 raw error 或 graph。
- Cost：Agent fix 不进入 provider ledger，但被独立上限约束；所有真实 provider run 仍走
  estimate/confirmation/actual ledger。
- Concurrency：durable claim、current+provider CAS、唯一 operation/chain index 与 atomic
  child linkage 共同防止重复 version/run/收费。
- Recovery：Agent in-flight 无法跨进程恢复，明确消耗 attempt；version 已提交后从 DB
  继续，不重放 Agent。
- Rollback：关闭 `HELIXFLOW_RUN_AGENT_FIX_ENABLED`；schema 和历史 audit 保留，既有
  retry/manual debug/cost gate 不变。
