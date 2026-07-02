# Task Plan

## Linked Issue

GH-65

## Spec Packet

- Product: `specs/GH65/product.md`
- Tech: `specs/GH65/tech.md`

## 实现任务

- [ ] `SP65-T1` Owner: frontend. 增加 port compatibility helper 与 connection drag state。Done when: output-only drag、compatible/incompatible candidate 计算有测试。Verify: `cd web && npm test -- app.test.tsx`
- [ ] `SP65-T2` Owner: frontend. 在 canvas 端口上接 pointer drag/drop 高亮和 cancel 行为。Done when: 不兼容释放不调用 API,取消拖拽无 op。Verify: `cd web && npm test -- app.test.tsx`
- [ ] `SP65-T3` Owner: frontend. 接入 `add_edge`、`remove_edge`、occupied input replace/cancel 批量 op。Done when: 连接/断线/替换均即时成版并刷新 state。Verify: `cd web && npm test -- app.test.tsx`

## 并行拆分

- helper 可先行。
- canvas pointer 与 API/store 修改需串行,避免 `graph-canvas.tsx` 冲突。

## 验证

- [ ] `SP65-T4` Owner: coordinator. 验收端口拖拽连线。Done when: web 测试和 SpecRail packet 校验通过。Verify: `cd web && npm test -- app.test.tsx && python3 checks/check_workflow.py --repo . --spec-dir specs/GH65`

## Handoff Notes

- 依赖 GH-59 `/versions/ops` 合并。
- Edge 路由美化不在本 issue。

