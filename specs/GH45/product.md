# GraphCanvas Selection, Shortcuts, And Clipboard Ergonomics

Status: draft
Issue: https://github.com/majiayu000/helixflow/issues/45
Locale: zh-CN

## 背景

GH42/GH44/GH48 已让 `GraphCanvas` 具备稳定导航、节点布局保存和可维护组件边界。下一步需要补齐桌面编辑器常见的选择、快捷键和复制能力，让用户能高效查看和分享当前 graph 信息。

## 目标

1. 用户可以框选区域内节点。
2. 用户可以用 Shift/Cmd/Ctrl 点击追加或移除选择。
3. 用户可以按 Escape 取消选择。
4. 用户可以按 Cmd/Ctrl+A 全选当前可见 graph 节点。
5. 用户可以按 Cmd/Ctrl+0 重置或适配视图。
6. 用户可以按 Cmd/Ctrl+C 复制选中节点摘要和 graph JSON 片段。
7. 多选时 inspector 显示 selection summary。
8. Pending proposal preview 下 selection 基于 preview graph。

## 非目标

- 不实现节点删除。
- 不实现手工新增节点或连线。
- 不实现粘贴为 graph mutation。
- 不改变后端 graph schema、store schema 或 API。
- 不改变 layout save、proposal apply/dismiss、run execution 语义。

## 用户场景

### 场景 1：框选一组节点

用户在编辑模式下拖出选择框，区域内节点被选中。用户也可以按 Shift/Cmd/Ctrl 拖拽进行追加选择。

### 场景 2：键盘快速选择

用户点击画布后，可以按 Escape 清空选择，按 Cmd/Ctrl+A 选择当前 graph 的所有节点，按 Cmd/Ctrl+0 把视图重置到 graph 区域。

### 场景 3：复制选中内容

用户选中一个或多个节点后按 Cmd/Ctrl+C，剪贴板得到稳定文本和 JSON 片段，便于粘贴给聊天、文档或 issue，不包含 provider secret 或本地绝对路径。

### 场景 4：审阅 pending proposal

当 pending proposal preview 存在时，选择、全选和复制都基于 preview graph，而不是 current graph。

## 产品需求

| ID | Requirement |
| --- | --- |
| PRD-01 | `GraphCanvas` 必须支持框选区域内节点。 |
| PRD-02 | Shift/Cmd/Ctrl 点击必须支持追加或移除单个节点选择。 |
| PRD-03 | Escape 必须清空当前 selection。 |
| PRD-04 | Cmd/Ctrl+A 必须选择当前 `drawGraph` 的所有节点。 |
| PRD-05 | Cmd/Ctrl+0 必须重置或适配视图。 |
| PRD-06 | Cmd/Ctrl+C 必须复制当前 selection 的文本摘要和 JSON 片段。 |
| PRD-07 | 快捷键不得拦截 textarea/input/contenteditable，也不得干扰 IME composition。 |
| PRD-08 | Clipboard 内容不得包含 provider secrets 或本地绝对路径。 |
| PRD-09 | 多选时 inspector 必须显示 selection summary。 |
| PRD-10 | Pending proposal preview 下 selection 必须基于 preview graph。 |

## 验收标准

- 框选区域能选中区域内节点。
- Shift/Cmd/Ctrl 点击能追加或移除选择。
- Escape 清空 selection。
- Cmd/Ctrl+A 选择当前 graph 所有节点。
- Cmd/Ctrl+0 更新 viewport。
- Cmd/Ctrl+C 写入稳定 clipboard 文本。
- 输入框、textarea、contenteditable 或 IME composition 时快捷键不触发 canvas action。
- Clipboard helper 会 redacts local absolute paths 和 token-like secrets。
- Pending proposal preview 下复制/全选包含 preview graph 节点。
- 前端测试覆盖 selection、shortcuts、clipboard 和输入框不拦截。
