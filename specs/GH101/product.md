# Product Spec

## Linked Issue

GH-101 (#101)

关联既有决策：本 spec 显式**反转** GH91 的非目标「保留现有 cost gate 和确认 modal」，需在 tech.md 记录理由与迁移不变量。

## 用户问题

HelixFlow 的 AI 视频产线目标是「用户说需求 → agent 出图 → 自动跑 → 看结果」的顺滑主流程。当前已把 agent 提案从「审批制」改成「自动应用成新 version + 成本阈值闸门」（本地已实现，`workbench_message_proposals.rs` / `sweep_support.rs`，77 后端测试通过）。但这条主流程还差两块，导致体验断裂：

1. **run 执行失败后无任何自愈**——provider 调用失败 / step 失败时，run 直接标 `failed` 并 `return Err`（`crates/run/src/executor.rs:234-245`），后台 spawn 仅 `eprintln!`（`crates/run/src/cost_gate.rs:116`）。用户只能看到「失败」，得手动重来。
2. **输出无法审查**——artifact 产生即终态，只有 `selected: bool`（`crates/store/src/run_records.rs:113`），没有「接受 / 打回」语义。自动应用+自动起跑后，用户失去了对最终产物的把关点。

## 目标

- **失败自愈**：run 执行失败后，在有界次数内自动重跑；重跑前若成本超阈值仍走确认。失败与每次重试对用户可见（不静默）。
- **输出审查**：为 artifact 增加 `review_state`（pending/accepted/rejected）。用户可接受或打回某个输出；打回可触发重跑（复用自愈重跑基座）。
- 两块能力共用统一重跑入口 `RunService::start_confirmed_run`（`crates/run/src/cost_gate.rs:82`）。
- 保留并文档化已实现的自动应用 / 成本阈值闸门 / version 回退（`version_routes.rs`）。

## 非目标

- 不替换 `RunService`、provider abstraction、cost ledger 记账机制。
- 不改 GH67 的 agent 提案校验重试（那是提案*生成*层，作用于 run 之前，`crates/agent/src/service.rs:41-155`）——本 spec 只管 run *执行*失败。
- 不实现「自动改图再跑」的智能修复（agent-refeed）作为默认路径；作为备选记录在 tech.md，留待后续 issue。
- 不改动多候选输出的 `select_output` 既有语义与「最近 run 作用域」约束（`artifact_routes.rs:125`）。

## Behavior Invariants

1. run 失败 → 自动重跑次数有上界（环境变量配置，默认值见 tech.md），耗尽后停在 `failed` 并保留 `error_json`。
2. 自愈重跑与首跑共享成本闸门：重跑成本 ≤ 阈值才自动起，> 阈值挂 `waiting_confirmation`。
3. 每次重试都 emit run event（`run.retry` 或等价），失败在 `error` 级可见，**禁止静默降级**（对齐 U-29）。
4. artifact 默认 `review_state = pending`；用户显式 accept/reject 才流转。
5. `reject` 一个输出可选地触发该 run 的重跑；`accept` 是终态，不可再改回 pending。
6. review 状态流转尊重 `latest_output_scope`：跨旧 run 的 artifact 不可 accept/reject（复用现有 409 逻辑，`artifact_routes.rs:263`）。
7. 已实现的自动应用管线（proposal→version）行为不变；本 spec 不回退它。

## 验收标准

- AC1：构造一个 provider 失败的 run，观察其在 ≤ N 次内自动重跑，run events 含每次重试记录；耗尽后 run.status = `failed` 且 `error_json` 非空。
- AC2：重跑成本 > 阈值时，重跑不自动起，run 挂 `waiting_confirmation` 并返回 pending_confirmation。
- AC3：新 artifact 的 `review_state` 初始为 `pending`；`POST /api/outputs/{id}/accept` 后为 `accepted`，`/reject` 后为 `rejected`。
- AC4：对已 `accepted` 的 artifact 再调 accept/reject → 幂等或 409（tech.md 定）。
- AC5：对非最近 run 的 artifact 调 accept/reject → 409。
- AC6：`reject` 且开启重跑选项时，触发一次新 run（复用 `start_confirmed_run`）。
- AC7：全部现有 77 后端测试仍通过（不回归自动应用管线）。

## 边界情况

- 重试计数如何持久化：复用同一 run 重跑 vs 派生新 run（parent_run_id）——tech.md 决策。
- run 被用户中断（`interrupted`）不触发自愈重跑（只对 `failed` 生效）。
- sweep（多 run group）失败自愈的粒度：整组重试 vs 单 run——tech.md 决策，默认只对推荐 run 生效。
- artifact 无 run 关联（node-only）时 review 语义。

## 发布说明

- 用户可见：run 失败会自动重试若干次；生成结果可「接受/打回」。
- 新环境变量（重试上界）需记入部署文档。
