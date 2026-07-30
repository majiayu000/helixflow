# Product Spec

## Linked Issue

GH-162

## 用户问题

#146 要求在删除 legacy proposal 契约前，用真实灰度数据证明 IntentPlan 的
success/clarify/error 分布、legacy 回滚演练和 v1 current graph 迁移完成度。但当前
Agent status 主要通过进程内 `EventBus` 传播：成功日志会在请求完成后写入 `messages`，
Agent/runtime error 在 `persist_agent_logs` 之前返回，contract mode、release/build
归属和 rollback 使用也没有结构化 durable record。现有数据因此无法可靠重建 #146 gate。

本功能增加 secret-free、append-only 的 contract observation，以及只读 evidence API。
它只负责生成可复核的原始事实，不替维护者决定通过阈值，也不删除任何 legacy 代码。

## 目标

- 每次 CreateWorkflow、ModifyWorkflow、DebugWorkflow turn 都有且只有一条 durable
  observation，记录 `intent` / `legacy` contract mode 和 terminal outcome。
- success、clarify、agent/runtime/compile error 即使请求最终返回错误也可审计；进程中断
  不能留下被统计为成功或静默消失的样本。
- observation 只保存稳定 ID、枚举、可空 release/build attribution 和时间，不保存
  prompt、graph、error text、provider response、credential、endpoint 或绝对路径。
- 提供 authenticated、只读 evidence API，按时间和 release 汇总 total、success rate、
  clarify/error reason 分布和 legacy rollback observations。
- 对所有 current workspace version 重新核验 canonical graph/migration 状态，不把旧
  assessment、`semantics_json` 非空或“存在迁移入口”误报为迁移完成。
- 缺 release/build、零样本、遗留 in-flight observation 或未迁移 current version 时
  显式返回 incomplete，不产生默认通过。

## 非目标

- 不删除 `HELIXFLOW_AGENT_INTENT_CONTRACT`、legacy proposal/read path 或其测试。
- 不决定 #146 的最小样本数、成功率、clarify/error 上限或允许的隔离策略。
- 不发布 release，不把测试数据、CI 或空数据库冒充生产灰度证据。
- 不增加用户 prompt、graph 内容、错误摘要或 provider payload 的 telemetry。
- 不增加外部 telemetry vendor、网络上报或跨实例聚合服务。
- 不在本功能提供 v1 migration apply 或 isolation 审批 UI；继续复用 #144 能力。

## Behavior Invariants

1. 仅 graph-edit modes 创建 observation；Chat 与 RunRequest 不进入 IntentPlan 删除 gate
   的分母。
2. user message 与 `started` observation 必须在同一 store transaction 创建，以
   `user_message_id` 唯一关联。任何可执行 Agent turn 都不能没有 observation。
3. observation 的 terminal outcome 仅允许 `success`、`clarify`、`error`，从
   `started` 单向推进一次；重复 completion 必须幂等返回原 record，冲突 outcome
   fail closed。
4. `contract_mode` 在 turn 开始时从 AppState 的 immutable request snapshot 写入
   `intent` 或 `legacy`，后续环境变量变化不能改写历史。
5. IntentPlan compile/apply 成功与 observation `success` 必须在 proposal/version/message
   的同一 transaction 提交；legacy proposal apply 遵循同一规则。不能先提交业务成功，
   再以 best-effort 写 observation。
6. clarify message 与 observation `clarify` 必须原子提交，reason code 使用 compiler
   已有稳定 code。clarify 不计作 success，也不计作 error。
7. Agent runtime/output failure和 compile/preview conflict 必须先把 observation 终态化为
   `error` 再返回 API error；若 observation 持久化失败，返回 store error，禁止打印
   warning 后丢弃。
8. server 启动、接受新请求之前，把旧进程遗留的 `started` observation 原子终态化为
   `error/PROCESS_INTERRUPTED`。重启重放不得重复计数。
9. reason code 只能来自代码内声明的枚举/稳定上游 code。不得把 `AgentError` 文本、
   validation message、prompt 或 provider response 写入 observation。
10. `release_id` 与 `build_revision` 从启动时严格校验的可选配置快照取得。缺失时数据库
    保持 `NULL`；evidence API 把它们列为 unattributed，不能归入任何正式 release。
11. legacy rollback evidence 只由真实 `contract_mode=legacy` observation 构成；仅设置
    flag、启动进程或运行 unit test 不算 rollback event。
12. evidence filter 使用闭区间 `since`、开区间 `until` 的 UTC 时间窗口；非法时间、
    `since >= until`、未知 release 格式必须返回 400，不得悄悄扩大查询。
13. success rate 的分母为同一 filter 下 terminal `intent` observations；
    `success / (success + clarify + error)`。API 同时返回原始整数，调用方不得只依赖
    浮点值。
14. 零 IntentPlan 样本、unattributed 样本、窗口内 `started` 样本均在 limitations 中
    显式呈现。API 不输出 `passed=true`，因为阈值属于 #146 的维护者 gate。
15. migration evidence 对每个 workspace 的 current version 读取并校验实际 graph file，
    复用 #144 canonical migration evaluator。只有全部 current versions 为
    `already_migrated` 且至少存在一个 current version 时，`migration.complete=true`。
16. `migratable`、`needs_resolution`、`failed`、graph missing/hash mismatch 或无 current
    version 都是不完整状态。历史 assessment 只提供审计上下文，不覆盖 current graph
    的重新核验结果。
17. 本功能不声明 `approved_isolation`。在维护者批准隔离数据模型和策略前，API 返回
    `approvedIsolation=0` 与 capability limitation；禁止把 unresolved 当作已隔离。
18. API 受现有 auth middleware 保护，响应只含 count、rate、stable code、release/build
    identity 与时间。workspace ID、message/session ID、graph hash和文件路径不出现在聚合
    响应。
19. observation rows 随 workspace 删除级联；不得阻塞用户删除本地 workspace。正常运行
    不更新或删除 terminal rows。
20. 所有 store/API/restart 测试使用 fixture 数据并明确标记为测试；release gate 只能
    消费实际部署数据库产生的 evidence packet。

## 验收标准

- [ ] intent/legacy success、clarify、Agent error、compile error 均有结构化 durable
      observation 和稳定 reason code。
- [ ] user message/start、proposal or clarify/terminal observation 分别满足原子性；
      重复 completion 与进程重启不重复计数。
- [ ] observation 写入失败显式返回错误；业务成功不能在 observation 缺失时提交。
- [ ] evidence API 支持时间/release filter，返回 raw counts、success rate、
      clarify/error reason distribution、legacy rollback observations 和 limitations。
- [ ] release/build attribution 缺失为 `null` 且不能被正式 release filter 命中。
- [ ] migration evidence 重新核验 current graph，区分 already migrated、migratable、
      needs resolution、failed、missing/currentless，零 workspace 不报告完成。
- [ ] API/DB 不包含 prompt、graph、raw error、credential、endpoint、provider response
      或绝对路径；安全回归测试覆盖恶意输入。
- [ ] Rust workspace fresh format/check/test 与 `git diff --check` 通过。

## 发布与回滚

先随下一个真实 release 发布 observation schema 和只读 evidence API。灰度部署必须显式
设置 `HELIXFLOW_RELEASE_ID` 与 `HELIXFLOW_BUILD_REVISION`，否则样本只进入
unattributed，不可用于 #146。回滚本功能时可停止读取 evidence API，但不能删除或改写
历史 observation。legacy contract 仍由原开关控制；本功能上线本身不改变 contract mode。
