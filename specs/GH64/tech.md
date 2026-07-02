# Tech Spec

## Linked Issue

GH-64

## Product Spec

`specs/GH64/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Inspector | `web/src/components/graph-canvas-inspector.tsx` | 参数以只读 span 渲染 | 主要 UI 改动点 |
| Registry schema | `web/src/types.ts`, node catalog API | param schema 已包含类型/枚举/范围 | 驱动控件类型和本地校验 |
| Store/API | `web/src/api.ts`, `web/src/store.ts` | GH-59 新 endpoint 接收批量 ops | Inspector 通过 `set_param` 即时成版 |
| Tests | `web/src/app.test.tsx` | 覆盖画布和 proposal 基础交互 | 需要新增 Inspector 编辑用例 |

## 设计方案

### 1. Schema-driven controls

为 param schema 建立渲染映射:string -> text input,number/integer -> number input,enum -> select,seed/integer 可带 random button。未知 schema 使用 disabled display。

### 2. Local edit state

Inspector 保持本地 draft state。字段 blur 或点击保存时提交;首版使用显式保存按钮/Enter 提交,避免每次 keypress 成版。

### 3. Submit path

提交构造单个 `set_param` op,携带当前 `baseVersionId`,调用 GH-59 `/versions/ops`。成功后 store 用返回 workspace state 替换本地状态,清空 draft/error。

### 4. Validation and errors

本地校验 schema range/enum/type。后端 400 带 `opIndex` 映射到当前字段;409 显示“版本已更新,请刷新/重试”并保留 draft。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 控件类型 | Inspector renderer | web unit tests per schema type |
| P2 set_param 即时成版 | api/store/Inspector | app test asserts `/versions/ops` request and state refresh |
| P3 本地校验 | validation helpers | invalid range/enum test |
| P4 后端错误内联 | store error handling | 400/409 mocked response test |
| P5 unknown schema disabled | renderer fallback | unknown schema test |
| P6 undo/restore sync | store state subscription | state refresh test |

## 数据流

selected node + catalog schema -> Inspector draft -> local validation -> `POST /versions/ops` set_param -> workspace state response -> store update -> Inspector rerender.

## 备选方案

- 继续复用 manual proposal 表单:违背即时编辑目标,放弃。
- keypress 自动保存:版本刷屏且错误体验差,放到后续评估。

## 风险

- Security: 不使用 `innerHTML`;错误文本按普通 text 渲染。
- Compatibility: schema 缺失时必须回退只读。
- Performance: 大 graph 下 state refresh 仍沿用现有机制。
- Maintenance: 控件映射集中在 helper,避免散落。

## 测试计划

- [ ] Unit tests: schema control mapping、本地校验。
- [ ] Integration tests: Inspector submit success/400/409。
- [ ] Manual verification: prompt/seed/size 编辑和 undo。

## 回滚方案

隐藏编辑控件,恢复只读 Inspector;GH-59 endpoint 不受影响。

