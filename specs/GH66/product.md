# Product Spec

## Linked Issue

GH-66

## 用户问题

用户无法从画布直接添加或删除节点,也无法把复制的选区粘贴回画布。常见的节点编辑动作必须绕到手动 proposal 表单或系统剪贴板文本,效率低且不符合工作台预期。

## 目标

- 节点库面板可按类别/搜索浏览 registry catalog。
- 用户可拖入或双击节点定义,创建带默认参数和位置的节点 proposal。
- Delete 键删除所选节点及关联边,一次原子成版。
- 复制/粘贴选中子图时生成新 node id,保留内部边,并避免 id 冲突。

## 非目标

- 不做自定义节点、子图打包或模板市场。
- 不做自动布局优化。
- 不绕过 pending proposal gate。

## Behavior Invariants

1. 节点库展示 catalog 中可用节点,支持搜索标题、类型和类别。
2. 添加节点时,新节点 id 在当前 graph 中唯一,参数使用 schema 默认值或安全空值,位置为 drop 点或视口中心。
3. 删除节点时,所有关联边随节点一起删除,preview proposal 中不留下悬挂边。
4. 复制选区只包含选中节点和选区内部边;粘贴时所有节点生成新 id,内部边重写到新 id。
5. 粘贴子图位置相对保留,整体偏移到当前视口或指针附近。
6. 已有 pending proposal 时,添加/删除/粘贴禁用或提示先处理 pending proposal。

## 验收标准

- [ ] 从节点库添加节点后可继续连线并运行,全程不用手动 proposal 表单。
- [ ] 删除带边节点一次成版,apply 后关联边消失,restore 可恢复。
- [ ] 复制 2 节点 1 边后粘贴,新子图 id 不冲突且内部边正确。

## 边界情况

- catalog 为空或加载失败:面板显示空/错误状态,不阻塞画布查看。
- 粘贴内容来自外部或旧版本:无效 payload 被拒绝,不创建 proposal。
- 删除全部节点会导致 graph validation 失败时,后端返回错误并展示给用户。

## 发布说明

前端编辑能力增强,依赖 GH59 op 模型和现有 proposal/version gate。无 schema migration。
