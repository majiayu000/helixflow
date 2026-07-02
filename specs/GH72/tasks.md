# Task Plan

## Linked Issue

GH72。

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`
- Analysis: `analysis.md`

## 实现任务

| ID | Owner | Depends on | Task | Done when | Verification |
| --- | --- | --- | --- | --- | --- |
| SP72-T0 | maintainer | none | 完成 spec review 和 implementation readiness gate | `GH72` 的 product/tech/tasks 被维护者接受，并推进到 `ready_to_implement` | `python3 checks/route_gate.py --repo . --route implement --issue 72 --state ready_to_implement --json` |
| SP72-T1 | frontend | SP72-T0 | 引入 UI tokens 和 director shell 视觉基础 | `TopBar`/canvas/chat 使用统一 tokens，品牌为 `helixflow`，不改变行为 | `cd web && npm test -- app.test.tsx`; `cd web && npm run build` |
| SP72-T2 | frontend | SP72-T1 | 增加 `QueueLockReason` helper 并接入 TopBar | dirty/pending/provider/run/empty 等状态有单一 reason，button title/copy 一致 | `cd web && npm test -- app.test.tsx` |
| SP72-T3 | frontend/backend | SP72-T2 | 设计并实现 manual edit session + batch commit contract | move/set_param/add_edge/add_node/remove_node 可累积为 dirty ops；commit 创建新 version；discard 不写版本 | `cargo test -p helixflow-server`; `cd web && npm test -- app.test.tsx` |
| SP72-T4 | frontend | SP72-T3 | 将 layout draft 迁移为 `move_node` dirty op | 拖动节点显示 `EDITING · N CHANGES`；Queue 锁住；commit 后保存位置 | `cd web && npm test -- app.test.tsx` |
| SP72-T5 | frontend | SP72-T3 | 拆分 canvas toolbar/selection toolbar/node toolbar | select/pan/connect/add/import/library 工具显示；unsupported 工具有 disabled reason | `cd web && npm test -- app.test.tsx`; manual browser QA |
| SP72-T6 | frontend/backend | SP72-T3 | 节点 dialog + schema-driven field edit | direct edit 产生 `set_param` dirty op；Agent rewrite 走 proposal；unknown context fields 被拒绝 | `cd web && npm test -- app.test.tsx`; relevant server schema tests |
| SP72-T7 | frontend | SP72-T2 | Cost gate、output screening、failure repair UI refine | `2b`/`2c`/`2d` 状态从真实 run/output/error 数据渲染 | `cd web && npm test -- app.test.tsx` |
| SP72-T8 | frontend/backend | SP72-T3 | Template empty state | 空 workspace 显示模板/一句话入口；不会创建 fake demo graph | `cd web && npm test -- app.test.tsx`; workspace bootstrap tests |
| SP72-T9 | qa | SP72-T1-T8 | 完整 Workbench QA | open -> edit -> commit -> Agent proposal -> cost gate -> run -> output select -> failure repair 路径被人工验证 | browser screenshots + command output attached to PR |

## 并行拆分

可以并行，但必须保持文件所有权不重叠：

- Shell lane: `web/src/components/top-bar.tsx`, `web/src/styles.css`, `web/src/panels.css`, `web/src/icons.tsx`
- Canvas lane: `web/src/components/graph-canvas*.tsx`, `web/src/components/graph-canvas*.ts`, `web/src/canvas.css`
- Store/API lane: `web/src/store.ts`, `web/src/api.ts`, `web/src/types.ts`
- Backend lane: `crates/server/**`, `crates/graph/**`, `crates/store/**`
- Run/output lane: `web/src/components/run-panels.tsx`, `web/src/components/artifact-stage.tsx`, `web/src/chat-log.css`

Do not let two parallel agents edit `web/src/app.tsx` or `web/src/store.ts` at the same time. Those are integration points and should be owned by one lane at a time.

## 验证

Docs/spec packet verification:

- `python3 checks/check_workflow.py --repo .`
- `python3 checks/check_workflow.py --repo . --spec-dir specs/GH72`

Frontend implementation verification:

- `cd web && npm test -- app.test.tsx`
- `cd web && npm run build`

Backend implementation verification:

- `cargo test -p helixflow-server`
- Add narrower tests for any changed graph/store crate.

Manual QA:

- Open a real workspace.
- Create dirty edit by moving node.
- Confirm Queue locks and shows dirty reason.
- Commit to new version.
- Send Agent message based on latest version.
- Confirm run cost gate.
- Select an output.
- Trigger failed run path and create minimal fix proposal.

## Handoff Notes

- This packet is bound to `GH72`; implementation remains gated on spec approval and `ready_to_implement`.
- Existing open issues `#59`, `#64`, and `#66` overlap with parts of this work but do not cover the full Workbench redesign.
- Implementation should start with behavior state (`QueueLockReason`, edit session) before large visual churn.
- Node catalog/schema failures must fail closed in UI. Do not show all node types as a fallback.
- Preserve IME-safe Enter behavior from `ChatPane`.
- Preserve human confirmation before paid provider runs.
