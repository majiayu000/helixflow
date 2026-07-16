# Task Plan

## Linked Issue

GH-114（#114）

## Spec Packet

- Product: `specs/GH114/product.md`
- Tech: `specs/GH114/tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP114-T0 | maintainer | none | 审阅 product/tech/tasks，并在接受后把 GH-114 推进为 `ready_to_implement` | GitHub 上存在维护者设置的 `ready_to_implement`，SpecRail `implement` gate 返回 `allowed` | `python3 checks/route_gate.py --repo . --route implement --issue 114 --evidence artifacts/triage/issue-114-evidence.json --json` |
| SP114-T1 | frontend-test | SP114-T0 | 在 `web/src/app.test.tsx` 增加消息可见性回归测试：user、agent chat、system/error、reply+tool log、HTML-like text；扩展 sendMessage 测试为 store 更新后重渲染断言 | 新用例在旧 renderer 上可复现失败，并明确断言最终 markup，而不只检查 store | `cd web && npm test -- app.test.tsx`（实现前预期新增用例失败） |
| SP114-T2 | frontend | SP114-T1 | 在 `web/src/components/chat-pane.tsx` 接通单一 `MessageTimeline`，复用 `chatEntries` 分派 `MessageRow`/`AssistantTurn`；收窄 EditSessionWorkspace 并删除重复/失效 helper | user、agent、system 与 tool group 均可见且不重复；proposal/run/edit/IME 行为不回退 | `cd web && npm test -- app.test.tsx` |
| SP114-T3 | qa | SP114-T2 | 运行完整前端验证并更新 GH-114 对应 QA 证据；如浏览器运行时可用，执行一次真实发送可见性 smoke | 前端全测与 build 通过；QA tracker 不再把“store 收到”当作“界面可见” | `cd web && npm test`; `cd web && npm run build`; `python3 checks/check_workflow.py --repo . --spec-dir specs/GH114` |

## 并行拆分

本 issue 不并行执行。任务按 `SP114-T0 → T1 → T2 → T3` 串行推进：

- `SP114-T1` 独占 `web/src/app.test.tsx`，先形成失败证据。
- `SP114-T2` 独占 `web/src/components/chat-pane.tsx`；仅当 fresh test 证明需要时才触碰 `web/src/styles.css`。
- `SP114-T3` 只更新验证/QA 证据，不与实现任务并行写同一文件。

这样符合 W-01 先复现和 W-14 不共享可写文件，也保持“一个个修复”的顺序要求。

## 验证

- Gate：`python3 checks/route_gate.py --repo . --route implement --issue 114 --evidence artifacts/triage/issue-114-evidence.json --json` 返回 `allowed`。
- 复现：新消息可见性测试在修复前失败、修复后通过。
- Focused frontend：`cd web && npm test -- app.test.tsx` 通过。
- Full frontend：`cd web && npm test` 通过。
- Build：`cd web && npm run build` 通过。
- Spec packet：`python3 checks/check_workflow.py --repo . --spec-dir specs/GH114` 通过。
- All specs：`python3 checks/check_workflow.py --repo . --all-specs` 通过。
- Manual：若浏览器运行时可用，发送普通中文消息，确认 user/Agent/error 正文可见。

## Handoff Notes

- 当前阶段只完成 spec packet；实现必须等待 `spec_approval` 和 `ready_to_implement` 人审门。
- 修复根因是 renderer 未挂载，后端/store/schema 不在改动范围。
- 必须复用 `chatEntries`，不要新建平行消息归组器。
- EditSessionWorkspace 不能继续重复渲染 latest user message 或 tool logs。
- 消息正文继续作为 React text node，禁止引入 `dangerouslySetInnerHTML`。
- 保留 IME-safe Enter、Shift+Enter、busy composer、proposal、RunErrorCard 和 edit-session 现有测试。
- 本地 worktree 已有审计产物和 QA tracker 改动，均属于前序用户任务，不得回滚。
