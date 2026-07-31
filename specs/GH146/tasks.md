# Task Plan

## Linked Issue

GH-146

## Spec Packet

- Product: `specs/GH146/product.md`
- Tech: `specs/GH146/tech.md`

## Gate Dependency

`SP146-T0` 未完成前，T1–T5 全部 blocked。不得以先写删除代码、后补数据的方式绕过。

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP146-T0` | maintainer | v0.2.0 deployment | 收集至少 7×24h、20 个真实 intent terminal observations、3 个 UTC days，并发布 raw gate packet。 | SLO、attribution、migration、rollback 全部满足 product spec；smoke/fixture/synthetic 不计入。 | 重放 issue comment 中的 evidence queries 并重算全部率 |
| `SP146-T1` | agent | T0 | 删除 graph-edit `proposal.json` runtime read/validation/retry，固定 Agent output contract 为 IntentPlan。 | Agent public/runtime graph-edit API 不能产生或读取 legacy proposal；unsupported param 继续 typed fail closed。 | `cargo test -p helixflow-agent -p helixflow-compiler` |
| `SP146-T2` | server | T1 | 删除 env flag、AppState bool、normal graph-edit 与 run-fix legacy branches；新 observation 固定 intent。 | production 搜索无 switch/legacy Agent call；success/clarify/error transaction 不退化。 | `cargo test -p helixflow-server agent_contract_observation && cargo test -p helixflow-server run_agent_fix` |
| `SP146-T3` | store/docs | T2 | 保留历史 legacy observation 可读；更新当前 README/runtime/prompt docs 与 tests。 | 旧 rows 不改写；evidence 仍显示 rollback drill；当前 docs 不再指导写 proposal.json。 | `cargo test -p helixflow-store agent_contract_observation` |
| `SP146-T4` | release | T3 | 在 canary DB 副本完成删除版 ↔ v0.2.0 ↔ 删除版 binary rollback drill。 | 两个 binary 都能打开副本；current/version/evidence 完整；无 destructive migration。 | 保存脱敏 health/count/version continuity transcript |
| `SP146-T5` | review | T4 | full verification、security review、exact-head CI/threads/merge gate。 | 无 actionable finding；所有 gate 证据对应 exact head；CI green。 | Rust/Web 全量命令 + `git diff --check` |

## 提交边界

1. T0 只有外部运行证据，不改代码。
2. T1 Agent contract convergence 独立提交。
3. T2 server convergence 独立提交。
4. T3 compatibility/docs 独立提交。
5. T4 只产生本地临时副本与脱敏 transcript，不提交数据库、prompt、graph 或 secret。
6. implementation PR 使用 `Closes #146`、`Refs #143`。

## 当前状态

- releases：2/2；
- v0.2.0 deployment smoke：完成；
- actual-current migration：1/1 complete；
- legacy rollback drill：完成；
- restored Intent positive smoke：完成；
- 正式稳定性窗口：从 `2026-07-31T06:34:00Z` 起，尚未满足 7×24h / 20 samples /
  3 UTC days，因此 T0 仍 blocked。
