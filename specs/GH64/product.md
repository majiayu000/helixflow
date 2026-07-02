# Product Spec

## Linked Issue

GH-64

## 用户问题

选中节点后,Inspector 只能查看参数,不能直接编辑。用户必须打开手动 proposal 表单填写 JSON 才能改 prompt、seed 或尺寸,打断画布编辑流。

## 目标

- Inspector 根据 registry param schema 渲染编辑控件。
- 修改参数提交 `set_param` op,即时生成 pending proposal/新版本流程与现有 manual proposal 一致。
- 非法值在本地和后端都能给出明确错误,并在 Inspector 内联展示。
- seed 参数提供随机按钮,枚举参数使用下拉。

## 非目标

- 不做节点增删、连线拖拽或子图粘贴。
- 不绕过 proposal review gate;本 issue 不直接改写 current graph。
- 不引入自定义 schema 语言。

## Behavior Invariants

1. 选中单个节点时,Inspector 展示该节点所有 schema 参数的当前值和控件。
2. string/number/integer/boolean/enum 使用对应控件;未知类型只读展示并提示暂不可编辑。
3. 用户提交合法值后,系统创建 `set_param` proposal,画布显示 preview,版本历史和 pending proposal 状态同步刷新。
4. 提交时若当前 workspace 已有 pending proposal,操作被拒绝并提示先处理现有 proposal。
5. 本地校验能拦截明显类型/范围错误;后端校验失败时错误显示在对应字段或 Inspector 顶部。
6. 编辑不影响未选中节点,也不会改变 selection、layout draft 或 run 状态。

## 验收标准

- [ ] 选中节点修改 prompt/seed/尺寸后产生 `set_param` proposal。
- [ ] enum 参数渲染为下拉,越界数字被拒并提示。
- [ ] 已有 pending proposal 时编辑按钮禁用或提交后明确提示。

## 边界情况

- 用户编辑时 base version 变更:提交返回 conflict,Inspector 显示需刷新/处理 proposal。
- 参数值为空:按 schema required/类型规则处理,不得把空字符串偷偷转成 null。
- 布尔值和数字类型不得以字符串提交给后端。

## 发布说明

前端交互增强,复用既有 manual proposal 路由和 proposal review gate。无 schema migration。
