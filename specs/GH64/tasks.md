# Task Plan

## Linked Issue

GH-64

## Spec Packet

- Product: `specs/GH64/product.md`
- Tech: `specs/GH64/tech.md`

## 实现任务

- [ ] `SP64-T1` Owner: frontend. 为 param schema 增加控件映射与本地校验 helper。Done when: string/number/integer/enum/unknown schema 均有确定渲染和校验结果。Verify: `cd web && npm test -- app.test.tsx`
- [ ] `SP64-T2` Owner: frontend. 改造 `graph-canvas-inspector.tsx` 支持 draft、保存、seed random 和 disabled fallback。Done when: 选中节点可编辑 prompt/seed/size,未知 schema 只读。Verify: `cd web && npm test -- app.test.tsx`
- [ ] `SP64-T3` Owner: frontend. 接入 `POST /versions/ops` set_param 与 400/409 内联错误。Done when: 成功刷新 workspace state,错误保留 draft 并显示。Verify: `cd web && npm test -- app.test.tsx`

## 并行拆分

- helper/test lane 可先做。
- Inspector component 与 store/api 接入需串行,避免同文件冲突。

## 验证

- [ ] `SP64-T4` Owner: coordinator. 验收 Inspector 编辑。Done when: `cd web && npm test -- app.test.tsx` 与 `python3 checks/check_workflow.py --repo . --spec-dir specs/GH64` 通过。Verify: `cd web && npm test -- app.test.tsx && python3 checks/check_workflow.py --repo . --spec-dir specs/GH64`

## Handoff Notes

- 依赖 GH-59 `/versions/ops` 合并后实现。
- 不做节点增删/连线,这些属于 GH-65/GH-66。

