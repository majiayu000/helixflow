# Task Plan

## Linked Issue

GH-105 (#105)

## Spec Packet

- Product: `specs/GH105/product.md`
- Tech: `specs/GH105/tech.md`

## Implementation Tasks

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP105-T1 | frontend | none | 给 workspace fetch/bootstrap 增加 generation + AbortSignal | A→B 与 A→B→A 的迟到 completion 都被拒绝 | `npm test -- --run src/store-background.test.ts` |
| SP105-T2 | frontend | T1 | event subscription 与 workspace activation generation 绑定 | 旧 subscription 不推进新 workspace state/seq | focused event tests |
| SP105-T3 | frontend | none | 新增 dirty navigation coordinator | clean/commit/discard/cancel/commit-failure 状态机可测 | coordinator unit tests |
| SP105-T4 | frontend | T3 | workspace switch/undo/restore/create 接入 coordinator 与确认 UI | 所有导航入口共享决策，pending 防重 | App integration tests |
| SP105-T5 | frontend | none | 修正 ChatPane submit promise/draft 生命周期 | failure 保留，success 条件清空，IME 不回归 | ChatPane tests |
| SP105-T6 | frontend | none | 集中 CanvasCapabilities 并守住 mutation 入口 | view 只 select/pan/zoom | canvas editing/connection tests |
| SP105-T7 | qa | T1-T6 | 回归 forceRerun 和 Web 全量 | 现有 Queue contract 通过，全量绿 | `npm test`; `npm run build` |

## PR Split / Ownership

- PR A (`Refs #105`): T1-T2；ownership `web/src/store.ts`, `web/src/api.ts`, `web/src/store-background.test.ts`。
- PR B (`Closes #105`): T3-T7；ownership `app.tsx`, `chat-pane.tsx`, `graph-canvas*`, new focused helpers/tests。
- 两个 PR 串行；`store.ts` 和 `app.tsx` 不允许并行写。

## Verification

- `cd web && npm test`
- `cd web && npm run build`
- `python3 checks/check_workflow.py --repo . --spec-dir specs/GH105`
- `python3 checks/check_workflow.py --repo . --all-specs`

## Handoff Notes

- `forceRerun` 已存在于 `api.ts` / App tests，不新建替代字段。
- dirty edit session 已存在，必须复用。
- 第一 tranche 只做 request/event isolation；导航/draft/view guard 在第二 tranche close issue。
