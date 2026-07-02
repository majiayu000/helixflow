# Tech Spec

## Linked Issue

GH-65

## Product Spec

`specs/GH65/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Canvas | `web/src/components/graph-canvas.tsx` | 已有 pointer events、节点拖拽、端口渲染 | 端口拖拽入口 |
| Types/schema | `web/src/types.ts`, node catalog | ports 带 type/direction | 兼容高亮和 drop validation |
| Store/API | `web/src/store.ts`, `web/src/api.ts` | GH-59 endpoint 支持 add_edge/remove_edge | 直接成版路径 |
| Graph validation | backend graph service | 后端判定端口/类型合法性 | 前端校验只能做快速反馈 |

## 设计方案

### 1. Drag state

前端新增 connection drag state:{fromNode,fromPort,portType,pointer}. 只允许从 output port 开始。拖拽中根据 catalog 和当前 graph 计算 candidate input ports。

### 2. Compatibility feedback

兼容 input port 高亮为可落点,不兼容显示 disabled/blocked。前端规则只做 UX 快速判断,最终以后端 validate_graph 为准。

### 3. Commit op

释放到兼容 input port 时构造 `add_edge` op。输入端已占用时弹出 replace/cancel;replace 发送 `[remove_edge old, add_edge new]` 批量 op,保证原子成版。

### 4. Disconnect

edge hover/selection 后通过 context menu 或快捷键触发 `remove_edge` op。成功后 workspace state refresh。

### 5. Errors

400 显示端口级错误,409 显示版本冲突并建议刷新。取消拖拽或释放空白处不调用 API。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 compatible edge | drag/drop + API | web test add_edge request |
| P2 incompatible blocked | compatibility helper | web test no API call |
| P3 add/remove versions | store/api | mocked workspace state refresh |
| P4 occupied input replace/cancel | replace dialog | web tests for both choices |
| P5 undo/restore consistency | state refresh | existing version state test |
| P6 cancel no-op | drag state | pointer cancel test |

## 数据流

pointer down output port -> drag state -> candidate highlight -> pointer up input port -> optional replace decision -> `/versions/ops` add/remove edge -> workspace state refresh.

## 备选方案

- 仅打开表单预填 from/to:仍不是真正画布连线,放弃。
- 前端直接改本地 graph 再后台保存:会产生前后端分叉,放弃。

## 风险

- Security: 不涉及 secret;错误文本按 text 渲染。
- Compatibility: pointer/touch 行为需不破坏节点拖拽。
- Performance: 大图高亮需 memoize port compatibility。
- Maintenance: replace 行为需和后端单输入端规则保持一致。

## 测试计划

- [ ] Unit tests: compatibility helper、occupied input replacement op。
- [ ] Integration tests: drag add edge、blocked incompatible、disconnect。
- [ ] Manual verification: mouse drag、cancel、undo。

## 回滚方案

隐藏端口拖拽 handlers,保留旧表单/API 路径。

