# Tech Spec

## Linked Issue

GH-101 (#101)

## Product Spec

见 `specs/GH101/product.md`。本 spec 覆盖两块缺失能力：run 失败自愈、输出审查，并文档化对 GH91 决策的反转。

## Codebase Context

事实来源（行号基于当前工作区，含未提交改动）：

**Run 生命周期**
- 状态枚举 `RunStatus { Queued, Estimating, WaitingConfirmation, Running, Succeeded, Failed, Interrupted }`：`crates/run/src/lib.rs:34-56`。DB 存字符串 `RunRecord.status`：`crates/store/src/run_records.rs:65`。
- DAG 执行器 `execute_created_run`：`crates/run/src/executor.rs:80-263`。失败聚合为 `first_error`（`:109,163-167`），失败 step 写 `error_json`（`:210-218`），run 标 `Failed`+emit `run.failed`（`:234-244`），随后 `return Err`（`:245`）。
- 后台起跑 `start_confirmed_run`：`crates/run/src/cost_gate.rs:82-128`。乐观锁抢占 → `ensure_run_steps` → `tokio::spawn` 跑 `execute_created_run` → 成功 `record_actual_costs`（`:115`）。失败分支目前只 `eprintln!`（`:116,119,122`）。
- sweep 版 `start_confirmed_sweep`：`crates/run/src/sweep_background.rs:11`。
- 失败查询 `latest_failed_workspace_run`：`crates/store/src/failed_run_records.rs:4-21`。
- 状态流转 store 方法 `update_run_status` / `update_run_status_if_current`：`crates/store/src/run_records.rs:206-264`。
- **重试/自愈：当前零实现**（`crates/run/` 无 retry/self_heal/refeed 命中）。

**成本闸门（前置 GH102 / PR #103 已合并）**
- `requires_run_confirmation`：`crates/server/src/sweep_support.rs:185-193`；阈值 `run_confirmation_threshold_usd()`（env `HELIXFLOW_AGENT_RUN_CONFIRMATION_THRESHOLD_USD`，默认 0）`:195`。

**Artifact / 输出**
- `ArtifactRecord`（无 status 列，仅 `selected: bool`）：`crates/store/src/run_records.rs:99-116`。`NewArtifact`：`:28-42`。
- 落库 `persist_artifact`：`crates/run/src/executor.rs:522`（step 产出后 `:343` 调用）。
- routes（`crates/server/src/artifact_routes.rs`）：`select_output`(`:19`)/`preview_output`(`:42`)/`download_output`(`:56`)/`artifact_content`(`:73`)，注册 `crates/server/src/main.rs:138-141`。
- 最近 run 作用域约束 `latest_output_scope`：`artifact_routes.rs:125`；跨旧 run 返回 409（`:263`）。

**提案应用流（已实现，不改）**
- `GraphService::apply_proposal`：`crates/graph/src/lib.rs:164-174`。store 乐观锁 `create_version_after_applying_proposal`：`crates/store/src/proposal_records.rs:209-311`。自动应用入口 `persist_and_apply_agent_proposal`：`crates/server/src/workbench_message_proposals.rs:16-76`。

**可复用范式**：GH67 有界回喂重试 `build_retry_message` / `max_proposal_rounds`：`crates/agent/src/service.rs:148-152`。

## Proposed Design（设计方案）

### 一、失败自愈（run 执行失败后有界自动重跑）

- **触发点**：`start_confirmed_run` 的后台 spawn 中，`execute_created_run` 返回 `Err` 时进入自愈循环。
- **策略（默认）**：**同图有界重跑**——针对 transient provider/step 失败，用同一 graph 派生新 run 重试。不做 agent 改图（那是备选，见下）。
- **有界**：新增 env `HELIXFLOW_RUN_MAX_RETRIES`（默认 1，即失败后最多再试 1 次；0 = 关闭自愈）。解析仿 `run_confirmation_threshold_usd()`。
- **成本闸门复用**：每次重跑前用现有 `requires_run_confirmation(&estimate)` 判定；超阈值则不自动重跑，run 挂 `waiting_confirmation`（不静默吞）。
- **派生 vs 复用**：**派生新 run**（新增 `runs.parent_run_id` 列 + `runs.attempt` 列，migration）。理由：保留失败 run 的 `error_json` 供 audit（对齐 W-37 失败轨迹留存），不覆盖。
- **可观测**：每次重试 `append_run_event` emit `run.retry`（含 attempt、上次 error）；耗尽后保留末次 `failed` + `error_json`，`error!` 级日志（对齐 U-29，禁止 warning+fallback）。
- **新增 store 方法**：`create_retry_run(parent_run_id, attempt)`（复制 graph/version 引用派生新 run 记录）。

### 二、输出审查（artifact accept/reject）

- **数据**：`ArtifactRecord` 增 `review_state: String`（`pending`/`accepted`/`rejected`，默认 `pending`），migration 加列 + 回填历史为 `accepted`（历史产物视为已接受，避免全量 pending）。改 `NewArtifact`/`ArtifactRecord`/`persist_artifact`（触发 vibeguard struct-field-change 清单）。
- **routes**（`artifact_routes.rs` 新增，注册 `main.rs`）：
  - `POST /api/outputs/{output_id}/accept` → `accept_output`
  - `POST /api/outputs/{output_id}/reject` → `reject_output`（body 可带 `{ "rerun": bool }`）
  - 仿 `select_output`（`:19`）实现，尊重 `latest_output_scope`（非最近 run → 409）。
- **状态机**：`pending → accepted`（终态）/ `pending → rejected`；`accepted` 不可变（幂等或 409，AC4 定为 409）；`rejected` 可再 `reject`（幂等）。
- **打回重跑**：`reject` 且 `rerun=true` → 调统一重跑基座 `start_confirmed_run`（与自愈同入口），复用成本闸门。
- **store 方法**：`set_artifact_review_state(output_id, state)`。

### 统一重跑基座

自愈重跑与 reject 重跑都经 `RunService::start_confirmed_run`（`cost_gate.rs:82`）+ `requires_run_confirmation`，保证成本闸门一致，避免两条重跑路径分叉。

## Product-to-Test Mapping

| Product AC | 测试 |
|---|---|
| AC1 有界自愈 | `run` crate: `retry_run_reheals_transient_failure_bounded` |
| AC2 重跑过成本门 | `server` `sweep_support`: `retry_over_threshold_waits_confirmation` |
| AC3 review 流转 | `server` `artifact_routes`: `accept_reject_transitions_review_state` |
| AC4 accepted 不可变 | `artifact_routes`: `accept_then_reject_returns_conflict` |
| AC5 跨 run 拒绝 | `artifact_routes`: `review_stale_run_returns_conflict` |
| AC6 reject 触发重跑 | `artifact_routes`: `reject_with_rerun_starts_run` |
| AC7 无回归 | 全量 `cargo test -p helixflow-server`（现 77 通过）+ `-p helixflow-run` |

## 数据流

失败自愈：`execute_created_run → Err` → 自愈循环（`cost_gate` spawn）→ `requires_run_confirmation` → [≤阈值] `create_retry_run` + 递归 `start_confirmed_run` / [>阈值] 挂 `waiting_confirmation` → emit `run.retry` → 耗尽则终态 `failed`。

输出审查：step 产出 → `persist_artifact(review_state=pending)` → 用户 `POST accept|reject` → `set_artifact_review_state` → [reject+rerun] `start_confirmed_run`。

## 备选方案

- **agent-refeed 自愈**（失败→回喂 agent 改图→重跑）：更智能但重，跨 run/agent crate 打通，成本/循环风险高。**本 spec 不做**，留后续 issue；默认同图重跑覆盖 transient 失败（占多数）。
- **artifact review 不加列，用 meta_json**：避免 migration 但无法索引/查询，放弃。
- **复用同 run 重跑**（不派生）：省一列但覆盖失败 `error_json`，违反失败轨迹留存，放弃。

## 风险

- migration 加列需回填历史 artifact 为 `accepted`，否则旧数据全 pending（中风险，回填 SQL 覆盖）。
- 自愈循环若成本判定有误可能自动烧钱——阈值默认 0 意味着「任何 >0 成本都要确认」，自愈默认只对 ≤阈值（免费/预付）run 生效，保守（低风险）。
- 派生新 run 增加 runs 表行数——可接受。
- `parent_run_id`/`attempt` 加列触发 store 层多处构造点同步（中风险，U-26 声明-执行完整性）。

## 测试计划

- `run` crate：自愈有界性、派生 run 保留父 error、耗尽终态。
- `server` crate：成本闸门下的重跑分支、review 状态机、跨 run 409、reject 触发重跑。
- 回归：现 77 server 测试 + run crate 测试全绿。
- 每个 checkpoint 跑 `cargo test`（对齐 W-42：run 生命周期是多步骤 artifact 工作流）。

## 回滚方案

- 代码：`git revert` 本 spec 的实现 commit；自愈由 `HELIXFLOW_RUN_MAX_RETRIES=0` 运行时关闭。
- migration：新列可空/带默认，回滚保留列不删（向后兼容 DB），或提供 down migration 删列。
- review：关闭 accept/reject route 不影响既有 select_output。
