# Tech Spec

## Linked Issue

GH-110 (#110)

## Product Spec

`specs/GH110/product.md`

## Codebase Context

| Area | Current truth | Extraction boundary |
| --- | --- | --- |
| `web/src/components/graph-canvas.tsx` | 800 行，持有画布 state 并内联 connection、node drag handlers | 控制器接收明确 state/callback，返回 handler；渲染仍由 GraphCanvas 负责 |
| `web/src/components/graph-canvas-connections.ts` | 已包含纯连接几何和 proposal helper | 新 controller 复用这些 helper，不复制规则 |
| `web/src/components/graph-canvas-layout.ts` | 已包含 position draft 纯函数 | node drag controller 复用这些函数 |
| `web/src/app.tsx` | 506 行，混合 workspace list、busy action、dirty navigation 和壳层渲染 | 新 hook 封装 action/navigation 生命周期，App 保留数据派生与 JSX |
| `web/src/dirty-navigation.ts` | 已有纯 plan/resolve state machine | 新 hook 只编排现有 helper，不创建第二套决策逻辑 |

## 设计方案

### 1. Canvas connection controller

新增 `graph-canvas-connection-controller.ts`，由 hook 持有 `connectionDrag` 和可见状态，返回 start/cancel/complete/disconnect handlers。依赖通过参数注入：active capability、draw edges、version、proposal callback、client-to-world 坐标函数。

控制器必须保留：

- mode 切换时取消 in-flight connection；
- input-only drop、port type、replace confirmation；
- proposal success/failure 可见状态；
- disconnect 也经过同一 capability gate。

### 2. Canvas node drag controller

新增 `graph-canvas-node-drag-controller.ts`，持有 drag ref 并返回 pointer handlers。控制器接收 selection、node map、zoom、base nodes、proposal callback 和 position draft setter。

控制器必须在 move capability 失效时清除 ref，pointer-up 时再次 fail closed，并继续通过 `buildMoveNodeEditInput` 创建现有 manual edit input。

### 3. App navigation/action hook

新增 `use-workbench-navigation.ts`，封装：

- workspace list 获取及错误；
- `busy`、history dialog、pending navigation、navigation busy；
- `runAction` 的 busy/refresh/error propagation；
- workspace/create/undo/restore 的 dirty-navigation 编排；
- workflow export 的 busy 生命周期。

hook 继续调用 `planNavigationRequest` / `resolveDirtyNavigation`，store action 通过参数注入。App 保留 bootstrap、event subscription、UI state derivation和 rendering。

## Product-to-Test Mapping

| AC | Verification |
| --- | --- |
| AC1-3 | `wc -l` production files；新增 controller/hook 不超过 650 |
| AC4 | `npm test`; `npm run build` |
| AC5 | `cargo test --workspace`; SpecRail checks |
| AC6 | 独立 reviewer 对 exact head 检查 extraction diff 与既有集成测试 |

## 风险与不变量

- handler 闭包依赖必须完整，不能因 stale state 改变 pointer 行为。
- controller 不得把 rejected proposal 变成静默成功。
- hook 不得在 Commit 失败后继续导航或清 dirty session。
- 拆分只移动既有规则；发现行为缺口时另立 issue，不顺手改语义。

## 回滚方案

所有提取均为前端内部模块，无 API 或持久化变更；可按 controller/hook 独立内联回滚。
