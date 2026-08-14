# Helixflow Workbench Layout Architecture

状态：P0 已在本地工作区实现并验证
日期：2026-08-11
范围：`web/` 客户端工作台壳层、Pane 布局、停靠、缩放、折叠与本地持久化

## 1. Objective

建立一套长期可演进、可测试、与业务内容解耦的 Workbench 布局基础设施。聊天、画布、运行、产物、历史、Inspector 等业务 Pane 只声明自身身份、能力和允许位置，不直接处理停靠、拖拽、尺寸、折叠或持久化。布局引擎只管理 UI topology，不读取或修改 workflow、run、artifact、proposal、cost 等业务状态。

这不是把当前两个底部卡片做成“可拖动”。目标是形成一个稳定的产品边界，使未来新增 Pane、调整默认布局、支持紧凑屏幕或增加布局配置时，不需要继续扩大 `app.tsx`、`run-panels.tsx` 或业务 store。

明确选择的状态所有权模型：**独立的客户端 `WorkbenchLayoutStore` 唯一持有布局 topology 和持久化 UI preference；现有 `useWorkbenchStore` 继续唯一持有后端权威的 workspace/domain state，两者禁止相互复制状态。**

## 2. Product Principles

1. 稳定区域，有限移动：用户在明确的 Zone 之间移动 Pane，不进入无限递归窗口管理。
2. 真实拖放：拖动期间显示可落点，pointer-up 只在合法目标上提交一次 move command。
3. 调整尺寸不是移动：Sash 只改变相邻 Zone 尺寸；Pane header drag 只改变位置或顺序。
4. 业务与布局分离：Pane 内容不知道自己当前位于左侧、右侧还是底部。
5. 可访问性等价：每个拖拽操作都有菜单或键盘命令替代方案。
6. 失败可见：损坏或不兼容的 layout snapshot 必须产生 diagnostic 并恢复默认布局，不能静默生成半合法状态。
7. 响应式降级由 renderer 决定：同一份 layout state 在窄屏变成 drawer/sheet，不由业务组件实现第二套移动逻辑。
8. 画布优先：Editor 始终占满可用空间；左右 Sidebar 与 Panel 都以 drawer/tray 覆盖画布，开关前后不改变 Editor 几何尺寸。
9. 结果归属节点：生成结果首先回填为节点内结果卡；点击结果卡只选择结果，不隐式改变工作台 topology，Artifact Viewer 由“查看”入口显式打开。

### 2.1 P0 implementation evidence

- Pure layout domain、reducer、typed errors：`web/src/workbench-layout/{types,defaults,reducer}.ts`。
- 独立 Zustand layout owner、v1→v2 migration、browser storage adapter：`store.ts`、`storage.ts`。
- Pointer drag、DOM geometry、Sash resize controller：`drag-controller.ts`、`dom-geometry.ts`、`resize-controller.ts`。
- React Zone/Container/Pane shell：`react/workbench-shell.tsx` 与 `workbench-layout.css`。
- Product registry binding：`web/src/workbench-pane-registry.tsx` 与 `web/src/app.tsx`。
- 2026-08-11 隔离提交快照验证：28 个 test files、229 个 tests 全部通过；TypeScript/Vite build 通过。完整工作区验证为 31 个 test files、239 个 tests；真实浏览器测得 Chat 开关前后 Canvas 均为 1512px 宽，Artifact Viewer 展开为 300px overlay 而 Canvas 仍为 1512px；同时验证结果卡点击、Run 默认 launcher、resize、drag、reload persistence 与 compact drawer/rail。

## 3. Current Evidence

以下证据来自 2026-08-11 的当前工作区。工作区存在其他未提交修改；本规范不把本地实现误称为远端已发布能力。

| Area | Evidence | Implication |
| --- | --- | --- |
| App entrypoint | `web/src/app.tsx` 直接排列 `ChatPane`、`GraphCanvas`、`ArtifactStage`、`HistoryPanel`、`WorkbenchDockPanels` | App 同时承担 domain composition 和 layout composition，需保留前者、移出后者 |
| Main shell | `web/src/styles.css` 的 `.wb-body` 和 `.chat-column` 固定聊天列宽；`web/src/canvas.css` 的 `.wb-canvas` 定义左/中/右/底 CSS Grid | Zone 已有雏形，但 topology 仍藏在 CSS 和 JSX 中 |
| Current dock spike | 工作区版 `web/src/components/run-panels.tsx` 内联 `DockLayout`、pointer drag、drop target、localStorage 和业务内容选择 | 属于 boundary creation prototype；不能继续扩展为总布局引擎 |
| Persistence | `helixflow.workbench.dock-layout.v1` 只保存 run/outputs 的 order、collapsed、position | schema 硬编码 Pane ID，缺少迁移、校验、尺寸和 Zone state |
| Domain state | `web/src/store.ts` 的 Zustand store 管理 workspace hydration、canvas、run、artifact、proposal 和 effects | Domain store 方向正确；不能塞入纯 UI layout preference |
| Canvas interaction | `graph-canvas-*` 已拆出 connection、node drag、navigation、selection 等 controller/helper | Workbench layout 应沿用 pure controller + injected adapter 的拆分方式 |
| Artifact viewer | `app.tsx` 通过 `canvas-stage--with-artifact` 和 `canvas-artifact-preview` 内联决定 split | 应迁移为 `artifact` View Container，不让 Canvas 持有 Viewer topology |
| History | `HistoryPanel` 当前是 popover/dialog 风格，并和 run panels 同文件 | 内容可保留；位置策略应由 Pane definition 决定 |
| Tests | `run-panels.test.tsx` 已覆盖折叠、按钮重排和 pointer docking；大量 app/store tests 保护业务语义 | 可迁移成 reducer、controller、adapter 和 React integration 四层测试 |
| Remote queue | 2026-08-11 远端无 open PR；open issues `#143`、`#146` 属于 runtime/schema legacy 收敛，不覆盖 Workbench layout | 布局基础设施需要单独 issue，不应夹进 runtime legacy 工作 |

## 4. Reference Models Considered

| Reference | Borrow | Do not copy | Source |
| --- | --- | --- | --- |
| VS Code Workbench | 固定 Workbench Parts/locations；View Container 与 View Pane 分层；SplitView/Sash 独立于内容；注册式贡献 | 全局 service locator、extension host、任意 editor grid、多窗口和多年兼容分支 | [`layout.ts`](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/browser/layout.ts), [`paneCompositePart.ts`](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/browser/parts/paneCompositePart.ts), [`viewPaneContainer.ts`](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/browser/parts/views/viewPaneContainer.ts), [`splitview.ts`](https://github.com/microsoft/vscode/blob/main/src/vs/base/browser/ui/splitview/splitview.ts), [`sash.ts`](https://github.com/microsoft/vscode/blob/main/src/vs/base/browser/ui/sash/sash.ts) |
| Higgsfield Supercomputer | 52/232px 导航 rail/sidebar、按需右侧 Viewer、12px resize hit area、稳定主工作区和受限内容宽度 | 聊天产品模型、营销/credit UI、远端任务架构、页面专属视觉细节 | [Live page inspected 2026-08-11](https://higgsfield.ai/supercomputer/16c3ec74-429b-4515-99f5-f1d67f18efb5) |
| Higgsfield Canvas | Canvas 是唯一常驻主表面；Chat 是右侧 overlay；工具、缩放与导航以浮动 chrome 出现；不设常驻底部 outputs | 具体品牌 UI、Higgsie、协作者与分享产品逻辑 | [Live canvas inspected 2026-08-11](https://higgsfield.ai/canvas/e7d4869d-2b14-4508-a402-d24935748d07) |
| TapNow Agentic Canvas | Canvas 是内容与结果的长期主表面；工具入口浮动；Agent 只读取用户明确选择的节点；运行产物回填为独立节点；窄屏与辅助面板按需覆盖 | 首页导航、订阅弹窗、模板营销结构、具体品牌视觉与模型商业逻辑 | [Explore the canvas](https://docs.tapnow.ai/en/docs/canvas/explore-the-canvas), [TapNow Agent](https://docs.tapnow.ai/en/docs/agent/tapnow-agent) |
| Current Helixflow prototype | Pointer capture、6px drag threshold、显式 drop zones、菜单式前移/后移 | 每个业务组件自建 `DockLayout`、用 DOM closest 查总容器、硬编码 run/outputs | 当前工作区 `web/src/components/run-panels.tsx` |

结论：借鉴“边界”和“交互合同”，不复制参考项目的规模产物。Helixflow 当前只需要四个稳定 Zone 和两层 Pane 模型，不需要第三方通用 docking framework。

## 5. Chosen Architecture

### 5.1 Two-level topology

```text
WorkbenchShell
├── TopBarPart                         product-owned, not dockable in P0
└── WorkbenchBody
    ├── PrimarySidebarZone             overlay drawer, resizable, collapsible
    │   └── ViewContainer(s)
    │       └── Pane(s)
    ├── EditorZone                     required, flex: 1
    │   └── CanvasEditor               P0 singleton
    ├── SecondarySidebarZone           optional overlay drawer, resizable, collapsible
    │   └── ViewContainer(s)
    │       └── Pane(s)
    └── PanelZone                      floating tray, resizable, collapsible
        └── ViewContainer(s)
            └── Pane(s)
```

四个 Zone 是稳定产品位置，不由 Pane 临时创建。View Container 是可移动的功能集合，Pane 是容器中的独立内容。

P0 默认布局：

```text
primarySidebar:  chat
editor:          canvas
secondarySidebar: artifact (hidden until useful)
panel:           execution [run, outputs] (default collapsed)
```

Sidebar 与 Panel 的 topology 仍是稳定 Zone，但 renderer 不再把它们当普通 flex row：左右栏展开为带 Sash 的浮动 drawer，Panel 展开为浮动 tray，收起后都只保留 launcher。这样仍可停靠、排序和持久化，又不会因辅助信息改变画布宽高。生成结果卡常驻其来源节点；点击结果只更新 selection，Artifact Viewer 只在用户显式打开时出现。

后续候选：

```text
primarySidebar:  nodeLibrary, workspaceExplorer
secondarySidebar: inspector, history
panel:           logs, diagnostics, costLedger
```

### 5.2 Boundary map

```text
product/app
  - app.tsx
  - workbench-pane-registry.tsx
  - product pane bindings and visibility rules

core/domain (pure TypeScript, no React/DOM/storage)
  - workbench-layout/types.ts
  - workbench-layout/commands.ts
  - workbench-layout/reducer.ts
  - workbench-layout/invariants.ts
  - workbench-layout/default-layout.ts

runtime/application
  - workbench-layout/store.ts
  - workbench-layout/selectors.ts
  - workbench-layout/drag-controller.ts
  - workbench-layout/resize-controller.ts
  - workbench-layout/migrations.ts

adapters/backends
  - workbench-layout/adapters/browser-storage.ts
  - workbench-layout/adapters/dom-geometry.ts
  - workbench-layout/adapters/animation-frame.ts

plugins/components
  - workbench-layout/react/workbench-shell.tsx
  - workbench-layout/react/workbench-zone.tsx
  - workbench-layout/react/view-container.tsx
  - workbench-layout/react/pane-host.tsx
  - workbench-layout/react/sash.tsx
  - workbench-layout/react/dock-overlay.tsx
  - workbench-layout/react/layout-command-menu.tsx

testing/headless
  - workbench-layout/testing/fake-storage.ts
  - workbench-layout/testing/fake-geometry.ts
  - workbench-layout/*.test.ts
  - workbench-layout/react/*.test.tsx
```

### 5.3 Dependency direction

```text
product pane bindings ───────► React layout hosts ───────► layout runtime
          │                                                │
          └────► existing domain store/API                  ▼
                                                  pure layout domain

browser storage / DOM geometry ─────► runtime ports
```

禁止反向依赖：

- layout core 不 import React、Zustand、DOM、localStorage 或 Helixflow domain types；
- `RunDock`、`OutputsStrip`、`ChatPane`、`ArtifactStage` 不 import layout store/controller；
- domain store 不保存 Zone、Pane order、尺寸或 collapse preference；
- storage adapter 不读取 run/artifact/workspace payload。

## 6. Core Data Model

P0 使用显式 Zone/Container/Pane 模型，而不是递归 split tree。它覆盖当前需求，并避免过早拥有任意窗口管理器。

```ts
export type ZoneId =
  | 'primarySidebar'
  | 'editor'
  | 'secondarySidebar'
  | 'panel';

export type ViewContainerId = string & { readonly __viewContainerId: unique symbol };
export type PaneId = string & { readonly __paneId: unique symbol };

export type WorkbenchLayoutDocument = {
  schemaVersion: 2;
  profileId: 'default';
  zones: Record<ZoneId, ZoneState>;
  containers: Record<string, ViewContainerState>;
  panes: Record<string, PanePlacementState>;
};

export type ZoneState = {
  visible: boolean;
  sizePx: number | null;       // editor is always null/flexible
  containerIds: string[];
  activeContainerId: string | null;
};

export type ViewContainerState = {
  id: string;
  zoneId: ZoneId;
  paneIds: string[];
  activePaneId: string | null;
  collapsed: boolean;
};

export type PanePlacementState = {
  id: string;
  containerId: string;
  collapsed: boolean;
};
```

布局文档只保存 identity 和 topology。标题、icon、renderer、allowed zones、minimum size、业务可见性由 registry 提供，不序列化函数或 React node。

### 6.1 Registry contract

```ts
export type PaneDefinition = {
  id: string;
  title: string;
  icon: IconName;
  defaultContainerId: string;
  defaultZone: ZoneId;
  allowedZones: readonly ZoneId[];
  singleton: true;
  canClose: boolean;
  canCollapse: boolean;
  minWidthPx?: number;
  minHeightPx?: number;
  availability: (context: WorkbenchPaneAvailability) => boolean;
};

export type PaneRenderer = (props: PaneRenderProps) => React.ReactNode;
```

Registry 分成两部分：

- metadata registry 可被 pure validation/default builder 使用；
- renderer bindings 只存在于 product React composition 层。

不得向所有 Pane 注入一个巨大的 `WorkbenchContext`。每个 renderer binding 只获取其需要的窄 props/selectors/actions。

### 6.2 Default capability matrix

| Pane | Default container | Default zone | Allowed zones | Collapse | Close |
| --- | --- | --- | --- | --- | --- |
| `chat` | `conversation` | primarySidebar | primarySidebar, secondarySidebar | yes | no |
| `canvas` | `editor` | editor | editor | no | no |
| `artifact` | `artifactViewer` | secondarySidebar | secondarySidebar, primarySidebar, panel | yes | yes |
| `run` | `execution` | panel | panel, secondarySidebar, primarySidebar | yes | no while active run exists |
| `outputs` | `execution` | panel | panel, secondarySidebar, primarySidebar | yes | yes |
| `history` | `history` | secondarySidebar | secondarySidebar, primarySidebar, panel | yes | yes |

关闭是 visibility，不卸载或删除 domain data。`availability` 只决定 Pane 能否渲染/打开，不能删除持久化布局记录；这样临时没有 outputs 时不会丢失用户位置偏好。

## 7. Commands, Events And Effects

所有变更进入 pure reducer。React handler 不直接拼 layout object。

```ts
export type LayoutCommand =
  | { type: 'zone/toggle'; zoneId: ZoneId }
  | { type: 'zone/resize'; zoneId: ZoneId; sizePx: number }
  | { type: 'container/move'; containerId: string; toZoneId: ZoneId; index: number }
  | { type: 'container/reorder'; containerId: string; index: number }
  | { type: 'container/toggleCollapsed'; containerId: string }
  | { type: 'pane/move'; paneId: string; toContainerId: string; index: number }
  | { type: 'pane/reorder'; paneId: string; index: number }
  | { type: 'pane/toggleCollapsed'; paneId: string }
  | { type: 'pane/activate'; paneId: string }
  | { type: 'pane/show'; paneId: string }
  | { type: 'pane/hide'; paneId: string }
  | { type: 'layout/reset'; profileId: 'default' };
```

Reducer 返回：

```ts
type LayoutResult =
  | { ok: true; state: WorkbenchLayoutDocument; events: LayoutEvent[] }
  | { ok: false; state: WorkbenchLayoutDocument; error: LayoutError };
```

错误分类：

| Error | Policy |
| --- | --- |
| unknown pane/container | recoverable，拒绝 command，记录 diagnostic |
| forbidden target zone | user-visible，drop overlay 标红并不提交 |
| required editor hidden | invariant violation，拒绝 command |
| duplicate singleton | invariant violation，拒绝 hydrate/command |
| corrupt persisted JSON/schema | storage diagnostic，使用 default document |
| storage quota/unavailable | diagnostic-only，内存交互继续工作 |
| DOM geometry unavailable | 取消当前 gesture，不改变 state |

## 8. Drag And Resize Runtime

### 8.1 Pane/container drag lifecycle

```text
pointerdown
  → capture(pointerId)
  → pending gesture
  → movement >= 6px
  → dragging
  → geometry adapter returns typed DropTarget
  → overlay previews target without mutating layout
  → pointerup on valid target
  → dispatch exactly one move/reorder command
  → persist committed layout
```

取消条件：`Escape`、`pointercancel`、window blur、pointerId mismatch、registry capability change、目标卸载。取消必须清理 capture、overlay 和 ephemeral drag state，不提交 layout。

不使用 HTML5 `draggable`/`dragstart` 作为核心路径。Pointer Events 对鼠标、触控和 pen 的行为更一致，也与现有 GraphCanvas pointer controller 一致。

Drop target 由 geometry adapter 计算并返回类型化结果：

```ts
type DropTarget =
  | { kind: 'zone'; zoneId: ZoneId; index: number }
  | { kind: 'container'; containerId: string; index: number }
  | { kind: 'pane'; containerId: string; index: number };
```

DOM class、`closest()` 和 bounding rectangle 不进入 reducer。视觉 overlay 只渲染 controller 给出的 `DropTarget`。

### 8.2 Sash resize lifecycle

- Sash 拥有 12px pointer hit area，视觉分割线保持 1px；
- `role="separator"`，设置 `aria-orientation`、`aria-valuemin/max/now`；
- pointermove 通过 animation-frame adapter 合并，避免高频 React render；
- preview size 在 runtime ephemeral state；pointerup 后 dispatch 一次 `zone/resize`；
- 双击恢复该 Zone 默认尺寸；
- 键盘方向键每次 10px，Shift+方向键每次 40px；
- 尺寸统一经过 registry/default constraints clamp。

默认约束：

| Zone | Default | Min | Max |
| --- | ---: | ---: | ---: |
| primarySidebar | 420px | 280px | `min(580px, 45vw)` |
| secondarySidebar | 420px | 300px | `min(720px, 55vw)` |
| panel | 240px | 120px | `min(520px, 55vh)` |
| editor | flexible | 420px desktop | flexible |

如果 viewport 无法同时满足所有 minimum，renderer 按 `secondarySidebar → panel → primarySidebar` 顺序进入 compact presentation，但不改写持久化桌面尺寸。

## 9. Rendering And Pane Lifecycle

`WorkbenchShell` 负责 Zone 结构，`PaneHost` 负责把 layout identity 解析成 product renderer。

渲染规则：

1. `editor` Zone 必须存在且可见。
2. 空且不可用的可选 Zone 不占空间。
3. Zone visible 但所有 Pane 暂不可用时显示空状态或自动隐藏；不删除 placement。
4. Collapse 默认保持 Pane mounted 并将内容从 accessibility tree 隐藏，保留 chat draft、scroll 等短期状态。
5. 跨 Zone 移动允许组件 remount；需要跨移动保存的 UI 状态必须归属于 Pane model/store，不能依赖偶然的 React local state。
6. Domain state 永远来自现有 backend-backed store；Pane remount 不触发重新创建 run、artifact 或 conversation。

P0 不引入 React portal keep-alive layer。只有真实性能或 draft-loss 证据出现时，才为特定 Pane 增加显式 `suspendPolicy`。

## 10. Persistence And Migration

### 10.1 Source of truth

| Contract | Source of truth | Scope |
| --- | --- | --- |
| Workflow/run/artifact/proposal | backend + current `useWorkbenchStore` projection | workspace/domain |
| Layout topology and sizes | `WorkbenchLayoutStore` | browser profile/device |
| Drag/resize preview | controller ephemeral state | current gesture only |
| Pane availability | product registry + current domain selectors | derived, never persisted |
| Default layout | `default-layout.ts` | code/config |

P0 使用注入的 `LayoutStorage` port，浏览器实现仍可基于 localStorage：

```ts
interface LayoutStorage {
  load(profileId: string): Promise<unknown | null>;
  save(profileId: string, document: WorkbenchLayoutDocument): Promise<void>;
  remove(profileId: string): Promise<void>;
}
```

Key：`helixflow.workbench.layout.v2.default`。

迁移流程：

1. 尝试加载 v2 并通过 Zod schema + invariants。
2. v2 不存在时读取 `helixflow.workbench.dock-layout.v1`。
3. 将 run/outputs position/order/collapsed 映射到 `execution` container 和 Pane placement。
4. 合并当前 registry default，补入 chat/canvas/artifact/history。
5. 写 v2；保留 v1 两个 minor release 作为 rollback bridge。
6. 遥测/diagnostic 确认迁移成功率后删除 v1 reader 和旧 key。

布局是设备 UI preference，P0 不写 backend database，也不随 workspace version 回滚。未来如果支持用户 profile sync，应增加独立 adapter，不改变 core document/reducer。

## 11. Responsive And Accessibility

### Desktop (`>= 960px`)

- 全部 Zone、Sash、pointer docking 可用；
- Secondary Viewer 按需出现；
- Panel 默认底部，可通过命令移动 View Container，而不是旋转整个 CSS Grid。

### Compact (`< 960px`)

- editor 保持主表面；
- primary/secondary sidebar 以 drawer 呈现；
- panel 以 bottom sheet 呈现；
- 禁用跨 Zone pointer drag，保留 Move 菜单；
- 同一 layout document 保留桌面位置和尺寸，compact open/close 属于 session UI state。

### Accessibility equivalence

每个 Pane header 必须提供：

- Collapse/Expand；
- Move to Primary Sidebar / Secondary Sidebar / Panel；
- Move Before / Move After；
- Hide（允许时）；
- Reset Location。

拖拽状态使用 live region 宣告“正在移动 X”“可停靠到 Y”“已移动/已取消”。Focus 在移动后回到 Pane header，隐藏 active Pane 时移动到同 Container 的下一个 Pane 或 Zone header。

## 12. Observability

P0 只需要低基数 diagnostic/event，不记录用户内容：

```text
layout.hydrate.started
layout.hydrate.succeeded { schemaVersion, migrated }
layout.hydrate.failed { reason }
layout.command.rejected { commandType, reason }
layout.drag.cancelled { reason }
layout.persist.failed { reason }
```

禁止记录 Pane 内的聊天、artifact URI、workspace name、node params 或 provider 数据。

## 13. Boundary Contract Matrix

| Contract | Owner | Allowed dependencies | Forbidden dependencies | Tests |
| --- | --- | --- | --- | --- |
| Layout state ownership | `workbench-layout/store.ts` | pure reducer, storage port | `useWorkbenchStore`, API client, DOM | `store.test.ts`, `ownership.test.ts` |
| Layout invariants | `reducer.ts`, `invariants.ts` | types, registry metadata | React, Zustand, browser globals | `reducer.test.ts`, `invariants.test.ts` |
| Pane metadata | `workbench-pane-registry.tsx` | stable IDs, product selectors | direct layout mutation, storage | `workbench-pane-registry.test.ts` |
| Pane rendering | `pane-host.tsx` + product bindings | React, narrow Pane props | storage, drag geometry, domain mutation outside injected actions | `pane-host.test.tsx`, `app.test.tsx` |
| Drag lifecycle | `drag-controller.ts` | geometry port, dispatch port | business types, direct localStorage, HTML5 DnD state | `drag-controller.test.ts` |
| Resize lifecycle | `resize-controller.ts` | rAF port, constraints, dispatch port | Pane content, domain state | `resize-controller.test.ts`, `sash.test.tsx` |
| Persistence | `browser-storage.ts`, `migrations.ts` | schema, migration functions | React, DOM layout, workflow state | `browser-storage.test.ts`, `migrations.test.ts` |
| Responsive rendering | `workbench-shell.tsx` | layout selectors, media adapter | rewriting persisted desktop sizes | `workbench-shell.test.tsx` |
| Errors/diagnostics | layout runtime | typed layout errors, diagnostic sink | chat/error banner domain store | `diagnostics.test.ts` |
| Compatibility v1 bridge | `migrations.ts` | old serialized type only | old React `WorkbenchDockPanels` | `migrations-v1.test.ts` |

## 14. Current-to-Target Component Map

| Current | Target | Rule |
| --- | --- | --- |
| `.wb-body` fixed flex composition | `WorkbenchShell` | App 只传 registry/bindings，不写 Zone JSX |
| `.chat-column` | `chat` Pane in `conversation` container | ChatPane 不拥有宽度和边框 |
| `.wb-canvas` hard-coded dock grid | Zone renderer | Grid/Flex 只是 adapter implementation |
| `WorkbenchDockPanels` | `execution` View Container | 迁移后删除 orchestration，保留纯内容组件 |
| `RunDock` | `run` Pane renderer | 保持业务 props/test，不读 layout |
| `OutputsStrip` | `outputs` Pane renderer | 保持 review actions，不读 layout |
| `canvas-stage--with-artifact` | `artifactViewer` container in secondarySidebar | ArtifactStage 不再由 Canvas split CSS 控制 |
| `HistoryPanel` popover | P1 `history` Pane 或保留 modal adapter | 不与 execution panels 共用 layout state |

## 15. Compatibility And Deletion Plan

| Path or shim | Why it exists | Owner | Keep until | Delete or converge when |
| --- | --- | --- | --- | --- |
| `helixflow.workbench.dock-layout.v1` reader | 保护当前本地用户偏好 | layout migrations | v2 发布后两个 minor release | migration success 已覆盖，回滚窗口关闭 |
| `WorkbenchDockPanels` | 当前工作区 prototype | frontend workbench | P0 registry/shell 接管 run/outputs | equivalent integration tests 通过且 v1 migration 落地 |
| `.wb-canvas` dock CSS areas | 当前左/右/底定位 | frontend workbench | WorkbenchShell Zone CSS 完成 | 无组件依赖 `dock-left/right/bottom` grid-area |
| `canvas-stage--with-artifact` | 当前 artifact inline split | artifact/workbench | artifact Pane 迁入 secondarySidebar | artifact selection/preview tests 在新 PaneHost 下通过 |
| `HistoryPanel` in `run-panels.tsx` | 历史文件组织遗留 | history/workbench | History 独立模块或 Pane 完成 | 文件不再混合 layout 与 history concerns |

## 16. Issue And PR Map

| Issue/PR | Contract served | Status 2026-08-11 | Gap or follow-up |
| --- | --- | --- | --- |
| `#143` | runtime/schema legacy convergence | open | 不承载 layout，避免扩大 scope |
| `#146` | legacy proposal deletion | open, needs_info | 不承载 layout |
| Workbench layout foundation | 本规范全部 P0 contracts | 尚未建 issue | 应单独创建 umbrella，并按 P0 tranche 拆 PR |

## 17. P0/P1/P2 Roadmap

| Priority | Work | Files/modules | Done when | Verification |
| --- | --- | --- | --- | --- |
| P0 | Pure layout document、commands、reducer、invariants、defaults | `web/src/workbench-layout/{types,commands,reducer,invariants,default-layout}.ts` | 所有 command 维持唯一 placement、required editor、allowed zones | `cd web && npm test -- workbench-layout/reducer.test.ts workbench-layout/invariants.test.ts` |
| P0 | Layout store、typed diagnostics、v1→v2 migration、storage port/adapter | `store.ts`, `migrations.ts`, `adapters/browser-storage.ts` | corrupt/unknown/old snapshots 都得到确定结果，无 domain state 被序列化 | `cd web && npm test -- workbench-layout/store.test.ts workbench-layout/migrations.test.ts workbench-layout/adapters/browser-storage.test.ts` |
| P0 | Registry、WorkbenchShell、Zone、PaneHost、Sash | `workbench-pane-registry.tsx`, `workbench-layout/react/*` | chat/canvas/run/outputs/artifact 使用注册式渲染；可折叠、可 resize、菜单可移动 | `cd web && npm test -- workbench-layout/react src/app.test.tsx && npm run build` |
| P0 | Pointer drag controller 和真实 drop overlay | `drag-controller.ts`, `dom-geometry.ts`, `dock-overlay.tsx` | 6px threshold、合法目标、cancel、单次 commit、pointer capture 均有测试 | `cd web && npm test -- workbench-layout/drag-controller.test.ts workbench-layout/react/dock-overlay.test.tsx src/components/run-panels.test.tsx` |
| P0 | 删除/收敛旧 orchestration | `app.tsx`, `run-panels.tsx`, `panels.css`, `canvas.css`, `styles.css` | `WorkbenchDockPanels` 和旧 grid area 无调用；Run/Outputs 行为不变 | `cd web && npm test && npm run build && git diff --check` |
| P1 | History/Inspector/Node Library 注册、Pane command palette、完整键盘 resize/move | product registry + panes | 每个 pointer action 有键盘/菜单等价路径 | `cd web && npm test -- workbench-layout/react/layout-accessibility.test.tsx && npm run build` |
| P1 | Compact drawer/sheet renderer | responsive adapter + shell CSS/tests | <960px 不丢失桌面 layout，主画布始终可达 | `cd web && npm test -- workbench-layout/react/workbench-shell-responsive.test.tsx && npm run build` |
| P1 | Named layout presets：Create / Debug / Review | defaults/profile service | preset 切换只改变 layout，不改变 domain/workspace version | `cd web && npm test -- workbench-layout/layout-profiles.test.ts` |
| P2 | 外部 Pane contribution API | typed contribution registry | 未知插件不能绕过 allowed zones、lifecycle、errors 和 tests | `cd web && npm test -- workbench-layout/contributions.test.ts && npm run build` |
| P2 | 可选 profile sync adapter | separate sync adapter | 冲突策略、版本迁移和隐私边界明确，不改 core reducer | `cd web && npm test -- workbench-layout/adapters/profile-sync.test.ts && npm test && npm run build` |

建议 P0 采用 3 个串行 PR，而不是一个大改：

1. `layout-domain-storage`：纯状态、迁移和 headless tests；
2. `layout-shell-sash`：注册、渲染、resize/collapse/menu move；
3. `layout-pointer-dock-convergence`：真实 drag overlay、业务 Pane 迁移、删除旧实现。

这些 PR 会触碰同一组 shell 文件，不适合并行写同一 worktree。

## 18. Validation Matrix

| Layer | What is proved | Required checks |
| --- | --- | --- |
| Pure unit | commands、constraints、invariants、defaults | reducer/invariants tests |
| Contract | registry capability、storage/migration、diagnostics | registry/storage/migration tests |
| Headless interaction | pointer threshold、drop resolution、cancel、resize clamp | fake geometry/storage/rAF tests |
| React integration | Zone mounting、collapse、focus、menu move、ARIA separator | React renderer tests |
| Product regression | chat send、canvas edit、run confirmation、output review、artifact preview | existing `app.test.tsx`, `run-panels.test.tsx` |
| Build/static | type contract、bundle、format whitespace | `npm test`, `npm run build`, `git diff --check` |
| Real browser | actual pointer drag/resize and reload persistence | Playwright/browser smoke path added in P0 PR 3 |

P0 acceptance scenarios：

1. Chat width resize → reload → width restored。
2. Run 从底部移动到右侧 → Viewer 合法布局 → reload 后位置恢复。
3. Outputs 暂时为空 → Pane 不渲染但 placement 保留 → 新 artifact 到达后仍出现在用户原位置。
4. 拖动未进入合法 target → pointer-up 后布局不变。
5. 拖动中 Escape/window blur/pointercancel → overlay 消失且布局不变。
6. localStorage 写失败 → 当前 session 仍可移动，diagnostic 可见。
7. v1 snapshot → 第一次加载生成等价 v2 → 第二次只读 v2。
8. compact mode 打开/关闭 drawer → desktop Zone 尺寸和位置不被覆盖。
9. Domain event 更新 run/artifacts → layout snapshot 内容不变化。
10. Move menu、keyboard resize 与 pointer path 产生相同 reducer command。

## 19. Non-Goals

- P0 不支持任意递归 split tree。
- P0 不支持浮动窗口、多浏览器窗口或 detachable Pane。
- P0 不把布局写入 workspace version、SQLite 或协作 canvas op-log。
- P0 不开放第三方插件 API。
- 不重写 Run/Artifact/Chat 的业务状态和 API。
- 不通过引入 GoldenLayout、FlexLayout 或 VS Code service framework 规避边界设计。
- 不保证跨 Zone 移动时保留所有组件 local state；重要状态必须显式归属。

## 20. Risks And Mitigations

| Risk | Mitigation |
| --- | --- |
| layout state 和 domain store 互相引用 | ownership test + forbidden import review |
| CSS Grid、React state、localStorage 三套 truth | topology 只来自 reducer state；CSS 只消费 selectors |
| Pane registry 变成 service locator | metadata/render binding 分离；每个 renderer 使用窄 props |
| drag 只在单元测试“看起来可用” | fake controller tests + real browser pointer smoke |
| 空 outputs 导致用户位置丢失 | availability 与 placement 分离 |
| resize 高频 render 卡顿 canvas | rAF coalescing，commit-on-pointerup，canvas performance regression test |
| responsive 覆盖桌面 preference | compact state session-only，不持久化转换后的尺寸 |
| 兼容代码永久存在 | 每个 bridge 有 owner、release window、delete condition |

## 21. Open Questions With Defaults

以下问题不阻塞 P0，未另行决定时采用推荐默认值：

1. Layout 是全局还是 workspace-specific？默认全局 profile；工作区只保存 transient active Pane。
2. Run 活跃时是否允许隐藏 Run Pane？默认允许 collapse，不允许 close；运行继续由 backend 管理。
3. Artifact Viewer 自动打开吗？默认首次生成 selected artifact 时打开 secondarySidebar；用户手动关闭后，本 session 不再强制打开。
4. Panel 是否整体移动到左/右？P0 移动 `execution` container，不改变 Panel Zone 的物理方向；P1 再评估 panel position command。
5. History 是 Pane 还是 modal？P0 保持 modal，P1 注册成可选 Pane，避免阻塞基础迁移。

## 22. Readiness

本架构的 P0 已在当前本地工作区实现并通过 unit、React integration、完整 web test/build 与真实浏览器交互验证。当前改动尚未提交；P1/P2 仍是后续工作，也不声称 Helixflow 已达到 VS Code 或 Higgsfield 的全部布局能力。
