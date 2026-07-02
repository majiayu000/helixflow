# Task Plan

## Linked Issue

GH-66

## Spec Packet

- Product: `specs/GH66/product.md`
- Tech: `specs/GH66/tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP66-T1` | web lane | GH59 | 新增 NodeLibrary 面板、catalog 搜索和默认 params/id 生成。 | 分类/搜索/add node payload 测试通过。 | `cd web && npm test -- app.test.tsx` |
| `SP66-T2` | server/graph lane | GH59/GH65 batch ops | 确认 batch AddNode/RemoveNode/AddEdge preview 支持删除/粘贴场景。 | 删除关联边、批量新增子图测试通过。 | `cargo test -p helixflow-graph && cargo test -p helixflow-server manual_proposal` |
| `SP66-T3` | web lane | `SP66-T1`,`SP66-T2` | 实现 Delete、copy/paste payload 校验和 id/edge rewrite。 | 复制 2 节点 1 边粘贴后 id 不冲突且内部边正确。 | `cd web && npm test -- app.test.tsx` |

## 并行拆分

NodeLibrary UI 与 server batch validation 可并行;copy/paste 接入依赖 batch ops。

## 验证

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP66-T4` | verification lane | `SP66-T1`-`SP66-T3` | 全仓测试和手工节点编辑冒烟。 | 添加、删除、复制粘贴、apply/restore 均通过。 | `cargo check --workspace && cargo test --workspace && cd web && npm test && npm run build` |

## Handoff Notes

- 粘贴 payload 是不可信输入,必须 zod/schema 校验。
- RemoveNode 已删除关联边,不要重复构造 remove_edge 导致 missing edge。
