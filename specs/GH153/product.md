# Product Spec

## Linked Issue

GH-153

## 用户问题

当前 run 失败后的自动自愈只会复制同一 `version_id` 与 `plan_json` 做有界重试。
当根因来自图本身（参数、连线、模型或 connector 不兼容）时，同图重试不会改变结果；
用户只能手动发送 debug 消息，才能进入现有 `DebugWorkflow` 修图路径。

本功能提供一个默认关闭的后台修图回路：仅在符合来源条件的 run 已失败、且同图重试
真正耗尽后，使用该失败 run 的脱敏诊断和对应版本图发起 `DebugWorkflow`，将合法修复
作为新的可回滚 version 提交，再对新 version 重新编译、重新估价并通过既有 cost gate。

## 目标

- 用 `HELIXFLOW_RUN_AGENT_FIX_ENABLED` 提供默认关闭的显式开关；关闭时现有 retry、
  sweep、cost gate 与 UI 行为逐字节兼容。
- 用 `HELIXFLOW_RUN_MAX_FIX_ATTEMPTS` 提供默认 `1` 的独立修图上限；不得复用或改写
  `runs.attempt` 的同图重试语义。
- 只处理 agent run 与 recommended sweep 成员；manual run、非推荐 sweep 成员、
  interrupted run 和输出打回触发的 force rerun 不自动修图。
- 修图使用精确 source run 的脱敏诊断与 source version 图，固定为
  `DebugWorkflow` / `FixError`，不经过关键词分类。
- 修复产物沿用 proposal auto-apply 与 immutable version 契约；修复后的 run 必须用
  新 version 重新编译、重新估价并经过既有 cost gate。
- 持久化每次修图 operation、阶段和派生对象，使重复终态通知、并发执行和进程重启
  不会重复 Agent 调用、重复 version 或重复 provider 执行。
- 通过持久化 `run.fix_attempt`、`run.fix_applied`、`run.fix_exhausted` 事件，让 UI
  明确区分同图 retry 与 Agent fix。

## 非目标

- 不改变 `HELIXFLOW_RUN_MAX_RETRIES` 的默认值、同图 retry 的 plan/estimate 复制语义，
  也不把 retry 次数与 fix 次数合并。
- 不修改 `classify_turn_mode` 的关键词顺序或用户手动 debug 路径。
- 不自动修复 manual run、非推荐 sweep 成员或 artifact review rerun。
- 不让 Agent 绕过 graph validation、version CAS、provider 选择或 cost confirmation。
- 不把 Agent 修图调用伪装成 runtime provider 费用；provider ledger 只记录修复后真实
  run 的估算与实际费用。
- 不处理远端 provider task handle 的重启恢复；该能力属于 GH-154。
- 不引入分布式队列、跨实例锁或无限后台循环。

## Behavior Invariants

1. `HELIXFLOW_RUN_AGENT_FIX_ENABLED` 缺失时为关闭；关闭时不得创建
   `run_fix_attempts`、Agent session、proposal、version、派生 run 或 fix 事件，也不得
   推进崩溃前遗留的 fix attempt/child preparation/outbox。既有未派发记录保持 quiescent，
   重新开启后才恢复；已经存在 dispatching/active/result_ready provider task 的 child
   仅由 GH-154 做计费安全收敛/materialization，不得由此触发新 fix/下游 run。
2. 仅在 fix 开关开启后解析 `HELIXFLOW_RUN_MAX_FIX_ATTEMPTS`：缺失时为 `1`，只接受
   非负整数。非法 UTF-8、负数、浮点或溢出值必须 fail-closed，持久化
   `run.fix_exhausted`，不得回退默认值；开关关闭时不解析该值，也不产生任何写入。
3. fix 上限按持久化 repair chain 计数，与 `runs.attempt` 完全独立。chain 必须保存
   `agent` / `recommended_sweep` 根来源；feature 开启时，recommended 成员必须在 sweep
   confirmation/claim transaction 中、任何成员进入 queued/running/dispatch 前持久化。
   值为 `0` 时不调用 Agent，直接记录 exhausted；任何 restart 或 retry 都不得重置计数。
4. fix 只在 source run 为 `failed`、同图 retry decision 明确为 `exhausted` 且没有
   `waiting_confirmation` retry child 时触发。retry 正在运行、等待确认、配置错误或
   基础设施结果不确定时不得抢跑 fix。
5. 普通入口只允许根来源为 `agent` 的 run；sweep 入口只允许数据库中已持久化的
   recommended member。retry child 必须继承 chain identity，不得靠已被改写的
   `trigger`/`group_id` 反推来源。非推荐 sweep、manual、interrupted、succeeded 和
   output-rejected force rerun 不得触发。
6. eligible run 的 failed terminal transition 必须与唯一 durable failure continuation
   同事务提交；retry child 或 fix claim 都消费该 continuation。进程在 failed commit 后、
   coordinator claim 前退出时，启动扫描必须准确恢复，不能漏修或误修。
7. 每次 attempt 先持久化唯一 operation 与 attempt index，再调用 Agent。重复终态通知
   必须返回已有 operation；同一 repair chain 的并发 claim 最多一个成功。
8. Agent request 固定使用 source run 对应的 workspace、version、安全 graph
   projection、当前受约束的
   provider catalog、`TurnMode::DebugWorkflow` 与 `AgentSkill::FixError`；不调用
   `classify_turn_mode`，不混入其他 workspace 或“最新失败 run”。
9. Agent 只接收 exact source run 及其 failed steps 的单行、截断、脱敏摘要。graph 与
   error 全部作为明确标记的 untrusted data，固定 system policy 禁止服从其中指令。
   不得把 raw
   `error_json`、stack、credential、auth header、signed URL、provider 原始响应或本地
   绝对路径写入 prompt、event、message 或 `run_fix_attempts`。
10. 修图输出必须通过现有 proposal/IntentPlan validation、graph preview、compiler 与
    server-side scope/diff gate。只允许修改 failed node 及其确定性依赖闭包，禁止删除或
    改写无关节点/边、workspace/provider 设置；需要超出范围时显式 exhausted，转人工。
    无输出、clarify、非法 proposal、Agent runtime 失败都算一次已消耗 attempt，保持
    source run 为 `failed`，并显式记录稳定 reason code。
11. 修复 version 必须是 source version 的 immutable child，保留可审计 proposal 与
    rollback；提交时同时 CAS workspace current version、nullable provider selection、
    实际 effective provider、GH-154 recovery scope fingerprint 与 catalog fingerprint。
    version apply 后，同样的 guard 还必须要求 workspace current version 仍等于
    `target_version_id`、nullable provider selection 仍等于快照，并在 child 创建、每次
    estimate、自动启动、稍后的用户确认和真实 dispatch 前重验；任一已变化则 fail closed，
    禁止执行已被 rollback/deselect 的 target 或用新 account/credential 执行旧 child。
    所有 fix-linked auto-start、manual confirm、claim 和 dispatch 还必须重验 feature
    enabled；关闭时返回 `FIX_DISABLED` 且不改 child。用户显式 hold/interrupt 仍允许。
12. candidate graph 文件发布、proposal/version/current pointer、attempt target linkage
    要么共同提交，要么执行引用感知 cleanup；cleanup 失败必须显式报错并交给启动
    reconciliation，不得遗留无记录的成功状态。
13. 修复后的 child run 不复用 source `plan_json`、`estimate_json` 或 cost ledger；必须
    从 target version 重新读取 graph、按 CAS 后的 provider 重新 compile/resolve/estimate。
14. child 创建先持久化 `child_preparing` 与稳定 child/step identity；每个 step 的 estimate
    使用 GH-154 提供的唯一 operation key。部分估价后崩溃必须恢复缺失项而非重复 ledger；
    全部估价完成后才原子进入 `waiting_confirmation`/可执行状态。
15. 修复后的 child run 必须经过既有 cost gate：估价未知或超过阈值保持
    `waiting_confirmation`；阈值内才可自动 claim/执行。Agent fix 本身不写 provider
    ledger，但受 fix 上限约束。
16. fix child 与 source run 的关系由 `run_fix_attempts` 持久化，不借用
    `runs.attempt`。child 失败后，必须先走自己独立的同图 retry，耗尽后才可能进入下一次
    fix；形成 `fix → run → retry* → fix` 的有界链。
17. Agent failure、clarify 或 invalid proposal 消耗 attempt 后，正常 coordinator 在仍有
    额度时立即 claim 下一 attempt；产生 child 后才等待该 child 的 retry exhausted。
    因此 `max=N` 严格表示整个 chain 最多 N 次 Agent 调用，N>1 不得只执行一次。
18. `version_applied` 后重复执行或重启恢复必须复用同一 target version，并幂等创建或
    返回同一 child run；不得再次调用 Agent。Agent 调用中进程退出时，该 in-flight
    attempt 视为已消耗，恢复逻辑不得假装成功或无痕重放。feature 已关闭时所有未派发
    attempt/child 状态保持不动；这类 quiescent child 在无 nonterminal provider task 时不占 workspace
    active-run slot，但自身的 execute/confirm 入口仍被 disabled guard 拒绝。用户显式
    hold/interrupt 是 disabled 状态下唯一允许的写入：必须在同一 transaction 把 attempt
    标为 `cancelled`、continuation 标为 `fix_completed/FIX_USER_CANCELLED`。无
    dispatching/active/result_ready provider task 时同时把 child 标为 interrupted；存在
    任一上述 task 时创建/升级 GH-154 `desired=interrupted` terminalization work item、
    撤销 DAG，child 暂留 running，最终由 settler 收敛为 interrupted。两条路径都永不恢复
    fix 或触发下一 fix。
19. attempt/decision 状态转换必须与唯一 event outbox 同事务提交；event 使用确定性
    dedupe key 写 `run_events` 后再发布。max=0、非法配置、重复 finalizer、重启和广播
    重试都只能产生一个 `run.fix_exhausted`。事件只包含稳定 ID、attempt/max、状态、
    reason code、target/child ID、confirmation 标志等安全字段。
20. `run.fix_applied` 只在 target version、child run 和完整 cost decision 均可查询后发出；其
    `requires_confirmation` 必须来自实际 cost gate。无法产出 child 的终态使用
    `run.fix_exhausted`，source run 始终保持原 `failed` 与 `error_json`。
21. UI 必须以不同文案/标识展示 retry 与 fix，并在 snapshot refetch 后保留未重复的 fix
    notices。fix child 等待确认时复用现有确认卡；UI 不把“version 已修复”呈现为
    “provider run 已成功”。
22. workspace/version/provider 并发变化、重复事件、服务重启及事件发布失败都不得产生
    多个 current version、多个 child run、重复收费或无限循环；所有失败均可从 DB
    记录和稳定事件重建。
23. GH-154 把“远端恢复后收敛为 failed”的 run 交给同一个 terminal failure
    finalizer；`interrupted`、仍在恢复的远端 run 与计费风险事件不得进入本修图回路。

## 验收标准

- [ ] 默认关闭与 `max=0/1/N`、非法配置、retry pending、retry exhausted、来源过滤均有
      确定性测试，现有 retry 测试保持通过。
- [ ] 开启后，agent run 与 recommended sweep 各覆盖一次
      `failed → retry exhausted → fix → new version → fresh child`；manual 与非推荐 sweep
      有负向测试。
- [ ] 测试证明修复 child 使用新 `version_id` 和重新生成的 plan/estimate，未知或超预算
      时停在 `waiting_confirmation`，阈值内才自动执行。
- [ ] 并发终态通知、重复 operation、version/provider CAS conflict 与候选文件故障注入
      均不产生重复 version、child run 或 ledger。
- [ ] restart 覆盖 recommended/non-recommended 在 failed commit 后、continuation claim 前的
      provenance 恢复，Agent in-flight attempt 消耗，以及 version/child/分步 estimate
      每个持久化窗口的幂等恢复。
- [ ] nullable default provider、默认 provider 改变、同 provider id 配置漂移均有 CAS
      测试；version applied 后和 child ready/等待确认后变更 account/credential 时，estimate、
      auto-start、manual confirm、dispatch 都必须拒绝旧 child。
- [ ] version applied 后 rollback/选择其他 current version 或改变 nullable provider
      selector 时，child create、estimate、confirmation 与 dispatch 全部拒绝。
- [ ] restart 前关闭 feature 时，claimed/version_applied/child_preparing/outbox 不产生
      新写入或 provider dispatch；重新开启后从同一 durable state 恢复。
- [ ] disabled restart 后仍可启动普通 manual run；quiescent fix child 的 confirm/dispatch
      返回 `FIX_DISABLED`。重新开启时若 workspace 忙则等待，current/selector 已变化则
      fail closed，绝不并行恢复。
- [ ] `child_preparing/estimating` 与 `child_ready/waiting_confirmation` 分别覆盖
      disabled → user interrupt/hold → reopen → reenable；attempt/continuation 均保持
      user-cancelled terminal，不补估价、不 dispatch、不 claim 下一 fix。
- [ ] disabled child 存在 `result_ready` 时，user interrupt 同事务终止 fix continuation
      并创建 GH-154 terminalization；reopen 只 materialize/settle 已付费 task，最终 child
      interrupted，不恢复 fix、不触发下一 attempt，workspace slot 在收敛前后均正确。
- [ ] 恶意 error 与 graph params 的 prompt-injection 测试证明 system policy 不被覆盖，
      scope/diff gate 拒绝无关节点删除、全图改写与 provider/workspace 设置修改。
- [ ] `max=2` 覆盖首次 Agent runtime failure/clarify/invalid 后的第二次 claim，以及达到
      上限后的 exactly-once exhausted。
- [ ] 脱敏测试覆盖 token、Authorization、signed URL、provider payload、绝对路径和超长
      多行错误；这些值不出现在 session context、event、message 或 attempt record。
- [ ] Web 测试覆盖 retry/fix 区分、snapshot notice 保留、等待确认和 exhausted 状态。
- [ ] Rust workspace、Web test/build 与 `git diff --check` 使用 fresh output 全部通过。

## 发布与回滚

首个版本保持 `HELIXFLOW_RUN_AGENT_FIX_ENABLED` 关闭，只验证 schema、事件与关闭态兼容。
灰度时先设较小 provider cost threshold 与 `HELIXFLOW_RUN_MAX_FIX_ATTEMPTS=1`，观察
attempt、applied、exhausted、confirmation 和 conflict 分布。回滚关闭开关后，未派发
attempt/child 保持 quiescent，不再创建 child 或 dispatch；已经派发的 remote handle 仍由
GH-154 安全收敛。历史 version、proposal、attempt、run 与 ledger 保留审计，不删除或改写。
