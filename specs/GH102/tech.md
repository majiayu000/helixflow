# Tech Spec

## Linked Issue

GH-102 (#102)

## Product Spec

见 `specs/GH102/product.md`。本 spec 独立交付两个相关但不互相隐式触发的行为：agent proposal 自动应用，以及 agent run request / seed sweep 的统一成本阈值起跑。

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Agent proposal orchestration | `crates/server/src/workbench_message.rs` | create/modify/debug turn 写 proposal 文件，创建 `pending` proposal 和 `proposal_pending` message，等待人工 apply/dismiss | 自动应用入口与响应契约的主改动点 |
| Proposal/version persistence | `crates/store/src/proposal_records.rs` | `create_proposal` 与 `create_version_after_applying_proposal` 是两个事务；后者已用 current-version 乐观锁原子更新 version/workspace/proposal | 直接顺序调用会在第二步失败时遗留 pending proposal；需要合并为一个事务边界 |
| Proposal apply API | `crates/server/src/proposal_routes.rs` | 人工 apply 读取 preview graph、生成 version 和 `proposal_applied` message | 作为兼容路径保留，并复用消息与 version history 语义 |
| Agent run request | `crates/server/src/sweep_support.rs` | 普通 run 与 seed sweep 都先创建 `waiting_confirmation`，无论估算金额大小 | 阈值内自动起跑与统一策略的主改动点 |
| Run lifecycle | `crates/run/src/cost_gate.rs`, `crates/run/src/sweep_background.rs` | request 阶段编译/估算/写 estimated ledger；`start_confirmed_run` / `start_confirmed_sweep` 用条件状态更新抢占并后台执行、写 actual ledger | auto-start 与 confirm-start 必须复用这些入口，避免重复执行和 ledger 分叉 |
| Confirmation API/payload | `crates/server/src/run_routes.rs`, `crates/server/src/workbench_payload.rs` | confirm/hold 路由只接受 `waiting_confirmation`；payload 从 estimate/ledger 展示成本 | 高成本路径继续使用，无需新增确认协议 |
| Web state | `web/src/store.ts`, `web/src/store-events.ts`, `web/src/components/chat-pane.tsx`, `web/src/components/run-panels.tsx` | Web 渲染 pending proposal 卡片和固定的 waiting-confirmation 文案 | 自动应用后需清理 pending preview，并正确展示 auto-start 或 waiting 状态 |
| Docs | `README.md`, `docs/AGENT_RUNTIME_PROVIDER_SPEC.md`, `docs/AGENT_RUNTIME_PROVIDER_VALIDATION.md`, `docs/PROMPT_DESIGN.md` | 文档仍把 agent proposal 描述为人工审批，未定义成本阈值配置 | 必须显式记录 GH-91 决策调整、默认值和验证方式 |

## 设计方案

### 1. 自动应用使用一个数据库事务

新增 store 级输入/返回类型与方法（命名以实现时现有约定为准，例如 `create_auto_applied_proposal_version`），在单个 SQL transaction 内完成：

1. 读取 workspace current version，并与 proposal 的 `base_version_id` 比较。
2. 插入终态为 `applied` 的 proposal record；不先暴露 `pending` 中间态。
3. 插入 immutable version，更新 workspace `cur_version_id`。
4. 插入 `proposal_applied` agent message，写入 version 引用，并把 message id 关联到 proposal。
5. 任一步失败整体 rollback。

server 在进入该事务前完成 proposal 校验、读取 base graph、应用 ops 和写 graph/ops/preview 文件。应用后的 graph path 使用 agent session 派生的稳定、安全相对路径，不依赖尚未生成的 proposal id。若事务因 version conflict 失败，文件可以作为未引用诊断文件保留，但数据库中不会出现 proposal/version/message 半状态。

agent logs 在自动应用事务之前持久化；若日志持久化失败，返回错误且不改变 current version。事务成功后 endpoint 返回 `proposal_applied` message，`proposal` 字段为空；不隐式创建 run。

现有人工 proposal apply/dismiss routes 保留，用于历史 pending 数据与非 agent 兼容流程；新的 agent turn 不再创建 pending proposal。

### 2. 乐观并发与错误语义

事务以 request 的 `base_version_id` 作为 `expected_current_version_id`。并发 proposal 中先提交者成功；后提交者收到现有 `VersionConflict` 映射的 HTTP 409。失败请求不创建 pending proposal，也不改变 current version。

graph 校验、文件写入、事务、估算和起跑错误都沿现有 `ApiError` 返回；后台执行错误由 run 终态/event 呈现。禁止把会造成错误状态或错误输出的问题改成 warning + fallback。

### 3. 统一成本确认策略

在 server 层保留一个纯策略函数判断 `CostSummary` 是否需要确认，普通 agent run request 与 seed sweep 共用：

- `USD` 且金额有限、非负：`amount > threshold` 时需要确认；相等视为阈值内。
- 非 `USD` 的正成本与非有限金额：保守地要求确认。
- 缺失 `HELIXFLOW_AGENT_RUN_CONFIRMATION_THRESHOLD_USD`：默认 `0` USD。
- 配置存在但无法解析、非有限或小于 `0`：返回明确配置错误，不静默回退。

普通 run：`request_agent_run` 完成编译、估算和 estimated ledger 后，阈值内调用既有 `start_confirmed_run`，否则返回 `waiting_confirmation` payload。

seed sweep：`request_sweep_plan` 完成整组估算后，阈值内把该 group 的 run ids 交给既有 `start_confirmed_sweep`，否则返回整组 `waiting_confirmation` payload。

confirm route 继续调用同一个 `start_confirmed_*` 入口。入口现有的条件状态抢占保证重复/并发 confirm 只有一次能从 `waiting_confirmation` 进入执行；其余请求返回冲突，不能重复调用 provider。

### 4. Web 与文档兼容

- `proposal_applied` message 到达后，store 清理旧 pending proposal/preview，并通过版本刷新显示新 current version。
- pending proposal 卡片保留兼容能力，但 agent 主流程文案不再承诺需要批准。
- run panel 根据 response 中是否存在 `pendingConfirmation` 展示“自动启动”或“等待确认”，不伪造金额。
- README 和 agent runtime 文档记录自动应用、version rollback、阈值变量、默认值、无效值行为，以及 proposal 不会隐式触发 run。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1-P2 自动应用、消息、回退引用 | store atomic method + `workbench_message` | store integration test；server create/modify response test；既有 version restore 回归 |
| P3 失败无阻塞 pending | store transaction rollback | 制造 version conflict，断言 proposal/version/message 数量与 current version 均未变化，随后合法请求成功 |
| P4 并发 proposal | store current-version compare-and-swap | 两个相同 base 的并发事务，断言一个成功一个 `VersionConflict` |
| P5 不隐式起跑、估算失败不启动 | `workbench_message`, `sweep_support` | proposal response `run=None`；估算失败时无 provider invocation |
| P6 阈值分支 | shared confirmation policy + run/sweep handlers | 普通 run 与 sweep 分别覆盖 `<`、`=`、`>` threshold |
| P7 起跑入口与 ledger 一致 | `start_confirmed_run`, `start_confirmed_sweep`, confirm routes | auto/confirm 两条路径的 run events、estimated/actual ledger 对比测试 |
| P8 错误可见 | `ApiError`, run terminal event | 配置错误、事务冲突、后台失败测试 |
| P9 配置默认与无效值 | threshold parser | missing=0；invalid/negative/non-finite 返回错误 |
| P10 兼容 | proposal routes, version routes, Web | 既有 proposal apply/dismiss、manual edit、restore 与 Web 全量回归 |

## 数据流

Agent proposal：`propose_graph_change` → validate prepared proposal → persist agent logs → read current base graph → apply ops in memory → write ops/preview/applied graph files → store atomic transaction（proposal applied + version + workspace current + message）→ response/message + workspace refresh。

Agent run request：`request_agent_run` → compile/estimate → create waiting run/steps + estimated ledger → confirmation policy → [阈值内] `start_confirmed_run` / [超阈值] pending confirmation → [confirm] 同一 `start_confirmed_run` → provider execution → events + actual ledger。

Seed sweep：`request_sweep_plan` → group estimate + per-run ledger → confirmation policy → [阈值内] `start_confirmed_sweep` / [超阈值] group pending confirmation → confirm route 进入同一 group start。

## 备选方案

- **先创建 pending，应用失败再标 failed**：补偿动作自身仍可能失败，且在两次事务之间可被其他请求观察并阻塞，放弃。
- **删除人工 proposal routes/UI**：会破坏历史 pending 数据和兼容流程，放弃；只让新的 agent 主流程自动应用。
- **把成本阈值放进 run crate**：run crate 不应读取 server 部署策略；request/execute 基座保持通用，策略留在 server orchestration。
- **proposal 自动应用后自动创建 run**：把编辑与付费执行耦合，超出本 issue 范围，放弃。

## 风险

- Security: graph/file 路径必须继续通过现有 data-dir 安全拼接，不接受用户提供的绝对路径；日志与错误继续执行 secret redaction。
- Compatibility: agent 新请求不再产生 pending proposal；历史 pending proposal 仍可人工 apply/dismiss。
- Performance: 新事务多写 proposal/version/message，但与现有三次独立写入同量级；graph 文件仍在事务外写，避免长事务。
- Maintenance: store 原子方法涉及 proposal、version、workspace、message 四类表；需用集成测试锁住声明-执行完整性。
- Operations: env 无效值从静默 0 改为明确错误，部署前需文档化并在启动/请求验证中可见。

## 测试计划

- [ ] Store unit/integration：原子成功、version conflict rollback、并发 one-winner、消息/proposal/version 引用完整。
- [ ] Server integration：create/modify/debug proposal 自动应用且 `run=None`；失败后下一请求不被 pending 阻塞。
- [ ] Run request matrix：普通 run 与 seed sweep 的 `<`、`=`、`>` threshold、missing/invalid env、重复 confirm。
- [ ] Ledger/events：auto-start 与 confirm-start 都有 estimated ledger，完成后有 actual ledger 和同构状态事件。
- [ ] Web：`proposal_applied` 清理 pending preview；auto-start/waiting-confirmation 展示；既有人工 proposal 卡片兼容。
- [ ] Full verification：`cargo fmt --check`、`cargo check --workspace`、`cargo test --workspace`、`npm test`、`npm run build`、SpecRail checks。

## 回滚方案

- 代码层通过 revert 恢复人工 pending proposal 与全量确认行为；不删除已经形成的 versions/proposals/messages。
- 紧急运行时可把 `HELIXFLOW_AGENT_RUN_CONFIRMATION_THRESHOLD_USD=0`，让所有正 USD 成本重新等待确认；免费 run 仍可自动起跑。
- 无数据库 migration；新增 store 方法使用既有表和状态，回滚不需要 schema down migration。
