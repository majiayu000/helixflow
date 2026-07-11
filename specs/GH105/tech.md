# Tech Spec

## Linked Issue

GH-105 (#105)

## Product Spec

`specs/GH105/product.md`

## Codebase Context

| Area | Current truth | Gap |
| --- | --- | --- |
| `web/src/app.tsx` | bootstrap 依赖 `selectedWorkspaceId`，直接 `setInitialState` | 无 generation/abort；dirty navigation guard 未统一 |
| `web/src/store.ts` | 管理 snapshot、message、run、provider 等异步 mutation | 多个 async completion 仅检查部分 workspace ID，无统一 request token |
| `web/src/store-events.ts` | event 已检查 workspace/run | 需纳入统一 generation 约束并覆盖快速切换 |
| `web/src/components/chat-pane.tsx` | draft 是组件 local state | submit 生命周期可能在失败前清 draft |
| `web/src/components/graph-canvas.tsx` | view/edit/review 共用 pointer/keyboard handlers | mutation guard 分散，view mode 容易漏入口 |
| `web/src/workbench-edit-session.ts` | 已有 dirty ops、commit/discard、Queue lock | 可复用，不新建第二套 dirty state |
| `web/src/api.ts` | Queue 已发送 `forceRerun` | 只需回归验证 |

## 设计方案

### 1. Workspace request generation

在 store 内维护不可序列化的单调 generation（或 request token）和当前 workspace ID。每次 workspace activation：

1. generation + 1。
2. abort 上一个 workspace 的 `AbortController`。
3. snapshot/canvas/message/provider 等 request 捕获 `{ workspaceId, generation, signal }`。
4. completion 在 mutation 前检查 token 仍 active；否则丢弃结果但不报当前 workspace 错误。

API fetch helper 应接受 optional `AbortSignal`。Abort 不是用户错误，不写入 error panel。仅比较 workspace ID 不够：A→B→A 时旧 A response 仍可能通过，因此必须比较 generation。

### 2. Event isolation

`applyEvent` 继续检查 payload `workspace_id`，并由 App/event subscription 捕获 activation generation。旧 subscription 的 callback 即使晚到，也必须在调用 store mutation 前失败。event seq 只在同 workspace + active run 上推进。

### 3. Dirty navigation coordinator

新增单一 helper/state machine，而不是在每个按钮复制 confirm：

```ts
type DirtyNavigationDecision = 'commit' | 'discard' | 'cancel';
type PendingNavigation =
  | { kind: 'workspace'; workspaceId: string }
  | { kind: 'undo' }
  | { kind: 'restore'; versionId: string }
  | { kind: 'create_workspace' };
```

执行规则：

- clean：直接执行。
- dirty + cancel：无状态变化。
- dirty + discard：先 `discardManualEdits()`，再执行目标。
- dirty + commit：await `commitManualEdits()`；仅成功且 edit session 清空后继续。
- pending 时禁用重复导航。

确认 UI 可以位于 App shell，但决策 reducer/helper 应独立可测。

### 4. Draft ownership

`ChatPane.submit` 必须：

- 捕获 trimmed draft；pending 时保留 input value。
- await `onSend`。
- 仅 resolve 后且 draft 未被用户修改时清空捕获值。
- reject 时保留草稿并让 store 的可见 error message 渲染。
- 保留现有 IME composition guard。

若 `onSend` 当前返回 `void`，改为 `Promise<void>`；禁止用 fire-and-forget 伪装成功。

### 5. View mode capability guard

集中派生：

```ts
type CanvasCapabilities = {
  select: true;
  pan: true;
  zoom: true;
  move: boolean;
  resize: boolean;
  connect: boolean;
  delete: boolean;
  paste: boolean;
};
```

所有 pointer/keyboard/clipboard mutation 入口先检查同一 capability。view mode 为 false 的 mutation 必须不调用 callback、不修改 draft。Edit/review mode 的现有行为保持。

## Product-to-Test Mapping

| AC | Tests |
| --- | --- |
| AC1 | `store-background.test.ts`: delayed A response after A→B and A→B→A |
| AC2-3 | navigation coordinator unit + App integration tests |
| AC4 | `chat-pane` component test: rejected promise preserves draft, resolved promise clears |
| AC5 | graph canvas editing/connection tests for view mode |
| AC6 | existing API/App force-rerun tests |
| AC7 | `npm test`, `npm run build` |

## 风险与不变量

- Abort signal 必须透传到 fetch，不得只在 completion 端忽略无限请求。
- generation 是前端运行时状态，不进入 API 或持久化 payload。
- dirty Commit 失败不得 fallback 到 Discard。
- view guard 不得阻止 pan/zoom/selection。
- 不修改后端 run/cost semantics。

## 回滚方案

- generation guard 可作为 store 内部实现回滚，不改变服务端 schema。
- navigation coordinator 可回滚为“dirty 时阻止导航”，仍不允许静默丢编辑。
- capability guard 可逐入口回滚，但 view mode 默认保持 fail closed。
