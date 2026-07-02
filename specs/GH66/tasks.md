# Task Plan

## Linked Issue

GH-66

## Spec Packet

- Product: `specs/GH66/product.md`
- Tech: `specs/GH66/tech.md`

## 实现任务

- [ ] `SP66-T1` Owner: frontend. 新增 node library panel、catalog search/category 渲染和 empty/error state。Done when: catalog 节点可搜索并选择。Verify: `cd web && npm test -- app.test.tsx`
- [ ] `SP66-T2` Owner: frontend. 实现 add_node drag/drop 与双击添加,含唯一 id 和位置计算。Done when: 添加节点即时成版,不打开手动 proposal 表单。Verify: `cd web && npm test -- app.test.tsx`
- [ ] `SP66-T3` Owner: frontend. 实现 Delete 批量删除选中节点及关联边。Done when: 删除带边节点一次成版,undo 可恢复。Verify: `cd web && npm test -- app.test.tsx`
- [ ] `SP66-T4` Owner: frontend. 实现 copy/paste app JSON、id remap、内部边重建和 invalid clipboard 错误。Done when: 2 节点 1 边选区粘贴后 id 不冲突。Verify: `cd web && npm test -- app.test.tsx`

## 并行拆分

- node library panel 可与 clipboard helper 并行。
- canvas/store/API 接入应串行整合。

## 验证

- [ ] `SP66-T5` Owner: coordinator. 验收节点库与复制粘贴闭环。Done when: web 测试和 SpecRail packet 校验通过。Verify: `cd web && npm test -- app.test.tsx && python3 checks/check_workflow.py --repo . --spec-dir specs/GH66`

## Handoff Notes

- 依赖 GH-59 `/versions/ops`。
- 自定义节点/子图打包不在本 issue。

