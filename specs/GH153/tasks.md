# Task Plan

## Linked Issue

GH-153

## Spec Packet

- Product: `specs/GH153/product.md`
- Tech: `specs/GH153/tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP153-T0` | run policy | GH154 | 增加 default-off fix flag、独立 max-fix parser 与 typed retry terminal decision；扩展 GH154 durable continuation，并在 sweep confirmation/claim、首次执行前持久化 recommended chain。 | 关闭态及遗留未派发 attempt 零写入；confirmation→execution 与 failed→claim crash 可恢复；retry child 继承 chain。 | `cargo test -p helixflow-run run_policy && cargo test -p helixflow-run self_heal` |
| `SP153-T1` | store | `T0` | 新增 repair chain，以安全 table rebuild 扩展 continuation fix states，增加 attempts/outbox/user-cancel finalizer；用 conditional CAS + unique attempt 单赢家。 | 旧 rows 无损迁移；user cancel 原子终止 fix，按 provider task 状态直接 interrupted 或创建 GH154 terminalization；不 claim 下一 fix。 | `cargo test -p helixflow-store run_fix` |
| `SP153-T2` | server security/agent | `T1` | 抽取 exact-run 脱敏 context与 safe graph projection；固定 DebugWorkflow/FixError/system policy；实现 deterministic scope/diff gate，处理 Agent invalid/clarify/failure。 | prompt 把 error/graph 当 untrusted data；恶意 params/error 不越权；无关 destructive diff 被拒绝；有余额时失败立即 claim 下一 attempt。 | `cargo test -p helixflow-server run_agent_fix` |
| `SP153-T3` | server/store version | `T1`,`T2` | 扩展 proposal candidate/auto-apply primitive，以 current version + nullable selection + effective provider + GH154 recovery scope + catalog fingerprint CAS 原子提交 fix proposal/version/message/attempt；接入 cleanup/reconciliation。 | NULL default、account/credential/catalog 漂移 fail closed；CAS conflict 不覆盖用户；fault injection 无孤儿 DB row。 | `cargo test -p helixflow-store run_fix && cargo test -p helixflow-server run_agent_fix` |
| `SP153-T4` | run cost gate | `T3` | 从 target graph fresh compile/resolve/estimate；在 child create/estimate/auto-start/manual confirm/claim/restart/dispatch 每次重验 feature、current target、nullable selector 与 provider scope。 | disabled confirm/dispatch 不改状态；rollback/deselect/换 account 拒绝；partial estimate 可恢复；ledger 幂等。 | `cargo test -p helixflow-run run_agent_fix && cargo test -p helixflow-server run_agent_fix` |
| `SP153-T5` | recovery | `T1`–`T4` | fix policy 先于 GH154 stale classification；关闭时 quiesce 并释放 busy slot，用户 hold/interrupt 原子终止 fix，remote task 交 GH154 收敛；开启时在正常 claim 下恢复。 | disabled manual run 可启动；result_ready cancel 跨 reopen 仅 materialize/settle 且不恢复 fix；reenable busy/current/slot guard 与 crash matrix 通过。 | `cargo test -p helixflow-server run_agent_fix_recovery` |
| `SP153-T6` | frontend | `T4` | store-events/types/UI 增加 `run.fix_*` notice、retry/fix 区分、snapshot 保留、refetch 与 pending confirmation 集成。 | fix attempt/applied/exhausted 可区分；notice 不重复；fix version 不误报 run success；现有 retry 文案不回归。 | `cd web && npx vitest run src/store-events.test.ts src/components/run-panels.test.tsx && npm run build` |
| `SP153-T7` | coordinator | `T0`–`T6` | 执行全量 fresh verification、安全/并发/恢复 review；确认 GH-154 边界与默认关闭发布说明。 | 全部 product invariants 有测试映射；无 raw secret/graph event；无 actionable review finding。 | `cargo fmt --all -- --check && cargo check --workspace --locked && cargo test --workspace --locked && (cd web && npm ci && npm test && npm run build) && git diff --check` |

## 顺序与提交边界

1. `SP153-T0` 在 GH-154 durable continuation 上建立 provenance，先保证现有
   retry 行为不回归。
2. `SP153-T1` 完成 durable truth；后续任务不得用内存 event bus 代替幂等记录。
3. `SP153-T2`、`SP153-T3` 串行完成 Agent 输入与 version transaction，避免共享
   `workbench_message_proposals.rs`。
4. `SP153-T4` 只消费 committed target；child/steps 先以 `child_preparing` 原子关联，
   不能在 proposal/version 未提交前创建 run，也不能用通用 startup 清理中断半估价 child。
5. `SP153-T5` 用真实重开 Store/AppState 的测试证明恢复，不用同进程函数重调冒充 restart。
6. `SP153-T6` 在后端 event payload 固定后实现。
7. `SP153-T7` 用当前 main fresh output 收口；不得引用历史 CI。

## Verification

实现完成后的标准命令来自当前根 `AGENTS.md`：

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked
cd web
npm ci
npm test
npm run build
cd ..
git diff --check
```

当前 main 已在 `a1ee3eb` / `#157` 退役 repo-local SpecRail automation，
`checks/check_workflow.py` 不存在，因此它不是本规格的可执行 gate。规格文件是设计记录；
implementation PR 仍须以 exact-head review、fresh GitHub Actions 和上述构建/测试为准。

## Handoff Notes

- `runs.attempt` 只表示同图 retry；fix 计数与 child lineage 只认 `run_fix_attempts`。
- `retry waiting_confirmation` 不是 exhausted，不能触发 Agent fix。
- 后台 fix 直接构造 `DebugWorkflow` / `FixError`，不修改或调用关键词分类。
- recommended provenance 只认 durable chain/continuation；不得从 retry 后的 trigger/group
  反推。
- 修复 version 提交必须同时 CAS current version、nullable provider selection、
  effective provider、GH-154 recovery scope 与 catalog fingerprint；estimate、确认和
  dispatch 前还要重验 target current version 与 selector，只做 apply-time CAS 不满足本规格。
- feature disabled 时未派发 fix 状态 quiescent；已有 remote handle 只允许 GH-154 计费
  安全收敛，不能触发下一 fix。
- error/graph/params 均视为 untrusted data；proposal 还必须过 server-side scope/diff gate。
- fix child 的 partial estimate recovery 依赖 GH-154 stable ledger operation key。
- `run.fix_applied` 意味着 target 与 child 都已持久化，不等于 child provider run 成功。
- GH-153 规格可以先合并，但 implementation 必须在 GH-154 的 durable continuation、
  recovery scope 和 stable ledger operation key 合并后开始；remote-active 或
  interrupted run 不能送入 fix。
