# Technical Spec: Seed Sweep Run Plans And Recommendations

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/28
Locale: zh-CN

## 输入资料

- `crates/run/src/cost_gate.rs`
- `crates/store/src/run_records.rs`
- `crates/store/src/lib.rs`
- `crates/registry/src/lib.rs`
- `crates/server/src/workbench_message.rs`
- `crates/server/src/workbench_payload.rs`
- `crates/server/src/run_routes.rs`
- `crates/server/src/main.rs`
- `web/src/types.ts`
- `web/src/components/run-panels.tsx`
- `web/src/store.ts`
- `web/src/app.test.tsx`

## 当前实现摘要

- Run layer 已有 `SweepPlan`、`SweepVariant`、`PendingSweep` 和 `confirm_sweep_runs`。
- `confirm_sweep_runs` 会校验 group、逐个执行 runs，并把 recommended run 的 output 标为 selected。
- Server 的 `RunRequest` 仍只创建单个 agent run，没有创建 seed sweep plan。
- `confirm_run` / `hold_run` route 只处理单个 run id。
- `PendingConfirmationPayload` 只包含 title、summary 和 cost。
- ConfirmModal 没有 run count、pending changes、interruptible 展示。
- Registry 只有 image mock 节点声明可选 `seed`，默认视频 mock 节点还不能 seed sweep。

## 设计决策

1. Seed sweep intent 在 server 侧识别，匹配用户文本中的 `seed` 或 `种子`。
2. Server 从当前 graph 生成 constrained sweep variants，只修改声明支持 `seed` 的 provider 节点，不接受 arbitrary batch scripts。
3. 第一版默认生成 4 个 variants，允许从用户文本中读取 2 到 6 的数字并 clamp。
4. 每个 variant 使用 deterministic seed 值；推荐输出选择最后一个 variant，避免引入未实现的 AI 评分。
5. ConfirmModal 的确认 id 使用 recommended run id；route 根据该 run 的 `group_id` 找到整组 runs。
6. Cancel/hold 对同一 sweep group 中所有 waiting runs 生效。
7. 新增 store helper 放在独立 `sweep_records.rs`，避免继续扩大 `run_records.rs`。
8. 新增 server helper 放在独立 `sweep_support.rs`，避免继续扩大 `workbench_message.rs`。

## Data Contract

`pendingConfirmation` 增加可选字段：

```json
{
  "id": "run_recommended",
  "title": "Seed sweep",
  "summary": "Seed sweep is waiting for confirmation.",
  "cost": { "amount": 1.68, "currency": "USD" },
  "runCount": 4,
  "pendingChanges": [
    "video.seed = 101",
    "video.seed = 202"
  ],
  "interruptible": true
}
```

现有单 run confirmation 仍可不发送这些可选字段。

## Backend Design

### Sweep plan helper

Add `crates/server/src/sweep_support.rs`:

- `is_seed_sweep_request(user_message)`
- `seed_sweep_run_count(user_message)`
- `build_seed_sweep(workspace_id, version_id, label, graph, user_message)`
- `seed_sweep_summary(pending, recommended_run_id, pending_changes)`

The helper:

- finds nodes with a registry `seed` param;
- clones the graph per variant and writes integer `seed`;
- validates every variant by relying on `RunService::request_sweep_plan`;
- returns deterministic pending changes for the confirmation payload.

### Registry

Add optional `seed` param to `video.mock.text_to_video` so the existing GH21 video workflow can use the Seed 试验 preset without changing node type.

### Routes

Enhance `confirm_run`:

- if target run has `trigger == "sweep"` and `group_id`, load all group runs;
- call `confirm_sweep_runs(&run_ids, &run_id)`;
- return all sweep artifacts and the recommended run payload.

Enhance `hold_run`:

- if target run has `trigger == "sweep"` and `group_id`, hold every waiting run in the group;
- return the target run payload with `pendingConfirmation=null`.

Add `Store::runs_for_group(group_id)`.

## Frontend Design

### Types

Extend `PendingConfirmationSchema` with optional:

- `runCount`
- `pendingChanges`
- `interruptible`

### UI

ConfirmModal renders optional sweep metadata in compact rows:

- run count
- pending changes list
- interruptible status

Existing approve/hold buttons continue to call the same route with the confirmation id.

## Product Requirement Mapping

| Requirement | Implementation Area | Verification |
| --- | --- | --- |
| PRD-01, PRD-03 | `sweep_support.rs`, `cost_gate.rs` tests | server/run tests |
| PRD-02 | `workbench_payload.rs`, `run-panels.tsx`, `types.ts` | web app tests |
| PRD-04, PRD-05, PRD-06 | `run_routes.rs`, `cost_gate.rs` | server/run tests |
| PRD-07 | existing run ledger plus route test | server/run tests |
| PRD-08 | `hold_run`, `runs_for_group` | server route tests |
| PRD-09 | existing run interrupt semantics | run interrupt tests |

## Risks

- Recommendation is deterministic, not quality-based. This is explicit for the first version.
- Sweep route returns one representative `run` payload while outputs include all artifacts. Future UI can add group progress.
- Adding optional `seed` to video mock schema changes accepted graph params but not provider behavior.

## Verification Commands

```sh
cargo fmt --check
cargo check --workspace
cargo test -p helixflow-run sweep
cargo test -p helixflow-server workbench_message
cargo test -p helixflow-server run_routes
cargo test --workspace
cd web && npm test -- app.test.tsx
cd web && npm run build
git diff --check
```

## Rollback Plan

- Remove sweep helper and route branching.
- Remove optional confirmation metadata fields from payload and UI.
- Remove optional `seed` param from video mock registry.
- Existing single-run confirmation path remains unchanged.
