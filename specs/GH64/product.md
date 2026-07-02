# Product Spec

## Linked Issue

GH-64

## 用户问题

用户选中节点后只能在 Inspector 里看到参数,不能直接编辑。改 prompt、seed、尺寸等参数必须绕到手动 proposal 表单填 JSON,不符合节点编辑器的常规体验。

## 目标

- Inspector 根据 registry param schema 渲染合适控件。
- 用户修改参数后提交 `set_param` op,即时生成新版本。
- 本地校验与后端结构化错误都以内联形式显示。
- 版本历史、画布、artifact state 在提交后刷新一致。

## 非目标

- 不实现节点增删、连线、拖拽端口。
- 不重做 Inspector 布局或完整属性面板设计。
- 不绕过 GH-59 的统一 op endpoint。

## Behavior Invariants

1. 文本、数字、枚举、seed 参数按 schema 渲染为可编辑控件。
2. 修改合法值并提交后,前端发送 `set_param` op 到 GH-59 endpoint,成功后工作区版本前进且选中节点显示新值。
3. 数字越界、枚举非法、空必填等本地可判定错误在提交前内联显示。
4. 后端返回 400/409 和 `opIndex` 时,Inspector 显示对应字段/全局错误,不得静默吞掉。
5. 只读/未知 schema 参数仍可显示,但不可编辑并说明不可编辑状态。
6. undo/restore 后 Inspector 与画布显示恢复后的参数。

## 验收标准

- [ ] 选中节点修改 prompt/seed/尺寸成功即时成版。
- [ ] 枚举参数渲染为下拉,越界数字被拒并提示。
- [ ] 后端 409/400 错误显示在 Inspector,不丢失用户输入。

## 边界情况

- 参数 schema 缺失、类型未知、默认值为空。
- 用户提交时当前版本已被其他编辑推进。
- seed 随机按钮生成值但未提交前不成版。

## 发布说明

Inspector 从只读变为可编辑。需要保留旧只读状态的视觉稳定性,避免误触自动保存。

