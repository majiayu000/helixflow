# Product Spec

## Linked Issue

GH-65

## 用户问题

画布不能用端口拖拽创建或断开连线。用户需要在手动 proposal 表单里手填 from/to 文本,这不符合节点编辑器的基本交互,也容易填错端口。

## 目标

- 从输出端口拖到兼容输入端口创建边,提交 `add_edge` op。
- 断开已有边时提交 `remove_edge` op。
- 拖拽过程中按 PortType 高亮兼容/不兼容端口。
- 输入端口已占用时给出替换/取消选择。

## 非目标

- 不做边的折线路由、美化或自动整理布局。
- 不做多边同端口输入;保持当前单输入连接约束。
- 不做节点新增/删除和子图粘贴。

## Behavior Invariants

1. 用户从 output port 开始拖拽时,所有兼容 input port 高亮为可落点,不兼容端口高亮为不可落点或无落点状态。
2. 拖到兼容且未占用 input port 后,系统创建 `add_edge` proposal,preview graph 立即显示新边。
3. 拖到不兼容端口或空白处时不创建 proposal,画布状态恢复。
4. 目标 input 已有连接时,用户必须选择替换或取消;替换以一个原子 proposal 删除旧边并添加新边。
5. 用户对已有边执行断开操作时,系统创建 `remove_edge` proposal;撤销/版本恢复可恢复该边。
6. 后端仍以 GraphService validation 为最终事实来源,前端高亮不得替代后端校验。

## 验收标准

- [ ] 类型兼容端口拖线成功且产生 preview proposal。
- [ ] 不兼容端口不能落线,并有清晰视觉反馈。
- [ ] 断线产生 proposal;apply 后版本历史可恢复。
- [ ] 已占用 input 的替换/取消行为可测试。

## 边界情况

- 拖拽期间 workspace version 变化:提交返回 conflict,画布提示刷新。
- pending proposal 存在时,拖线编辑禁用或明确提示先处理 proposal。
- 触屏/鼠标 pointer cancel 时必须清理临时连线。

## 发布说明

前端交互增强,复用现有 manual proposal 路由。无 schema migration。
