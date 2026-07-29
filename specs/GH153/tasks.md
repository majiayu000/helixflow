# Task Plan

## Linked Issue

GH-153

## Spec Packet

- Product: `specs/GH153/product.md`
- Tech: `specs/GH153/tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP153-T0` | run policy | GH154 | 增加 default-off fix flag、独立 max-fix parser与 typed retry terminal decision；为 eligible failed terminal 原子创建 durable work item，持久化 agent/recommended provenance。 | 关闭态零副作用；failed→claim crash 可恢复；retry child 继承 chain；manual/非推荐 sweep 负向测试通过。 | `cargo test -p helixflow-run run_policy && cargo test -p helixflow-run self_heal` |
| `SP153-T1` | store | `T0` | 新增 repair chain、failure work item、`run_fix_attempts`、event outbox migration/records、状态机、operation replay 与 child linkage。 | fix 计数不使用 `runs.attempt`；并发 claim 单赢家；max=0/非法/N>1 与重复 finalizer exactly-once；workspace 删除不被 FK 阻塞。 | `cargo test -p helixflow-store run_fix` |
| `SP153-T2` | server security/agent | `T1` | 抽取 exact-run 脱敏 context与 safe graph projection；固定 DebugWorkflow/FixError/system policy；实现 deterministic scope/diff gate，处理 Agent invalid/clarify/failure。 | prompt 把 error/graph 当 untrusted data；恶意 params/error 不越权；无关 destructive diff 被拒绝；有余额时失败立即 claim 下一 attempt。 | `cargo test -p helixflow-server run_agent_fix` |
| `SP153-T3` | server/store version | `T1`,`T2` | 扩展 proposal candidate/auto-apply primitive，以 current version + nullable selection + effective provider/config fingerprint CAS 原子提交 fix proposal/version/message/attempt；接入 cleanup/reconciliation。 | NULL default/provider config 漂移 fail closed；CAS conflict 不覆盖用户；fault injection 无孤儿 DB row。 | `cargo test -p helixflow-store run_fix && cargo test -p helixflow-server run_agent_fix` |
| `SP153-T4` | run cost gate | `T3` | 从 target graph fresh compile/resolve/estimate，持久化 `child_preparing`，复用 GH154 stable estimate operation keys 幂等补齐部分估价；完成后复用 confirmation/claim/execute。 | 每个 estimate crash window reopen 可恢复；target `version_id` 正确；unknown/over budget pending，within budget 自动执行；不重复 ledger。 | `cargo test -p helixflow-run run_agent_fix && cargo test -p helixflow-server run_agent_fix` |
| `SP153-T5` | recovery | `T1`–`T4` | AppState 启动恢复 work item/attempt/outbox；Agent in-flight 明确消耗，`version_applied` 补 child，`child_preparing` 补 estimate；消费 GH-154 common finalizer seam。 | provenance、Agent、version、每步 estimate、event publish crash matrix 全覆盖；不重放 Agent/version/ledger；interrupted/remote-active 被拒绝。 | `cargo test -p helixflow-server run_agent_fix_recovery` |
| `SP153-T6` | frontend | `T4` | store-events/types/UI 增加 `run.fix_*` notice、retry/fix 区分、snapshot 保留、refetch 与 pending confirmation 集成。 | fix attempt/applied/exhausted 可区分；notice 不重复；fix version 不误报 run success；现有 retry 文案不回归。 | `cd web && npx vitest run src/store-events.test.ts src/components/run-panels.test.tsx && npm run build` |
| `SP153-T7` | coordinator | `T0`–`T6` | 执行全量 fresh verification、安全/并发/恢复 review；确认 GH-154 边界与默认关闭发布说明。 | 全部 product invariants 有测试映射；无 raw secret/graph event；无 actionable review finding。 | `cargo fmt --all -- --check && cargo check --workspace --locked && cargo test --workspace --locked && (cd web && npm ci && npm test && npm run build) && git diff --check` |

## 顺序与提交边界

1. `SP153-T0` 在 GH-154 common finalizer 上建立 durable work item/provenance，先保证现有
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
- recommended provenance 只认 durable chain/work item；不得从 retry 后的 trigger/group
  反推。
- 修复 version 提交必须同时 CAS current version、nullable provider selection 与
  effective provider/config fingerprint；只做 current CAS 不满足本规格。
- error/graph/params 均视为 untrusted data；proposal 还必须过 server-side scope/diff gate。
- fix child 的 partial estimate recovery 依赖 GH-154 stable ledger operation key。
- `run.fix_applied` 意味着 target 与 child 都已持久化，不等于 child provider run 成功。
- GH-154 不阻塞 GH-153；它只需在未来把确实恢复为 failed 的终态送入同一个 finalizer，
  不能把 remote-active 或 interrupted run 送入。
