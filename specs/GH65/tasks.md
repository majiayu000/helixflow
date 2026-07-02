# Task Plan

## Linked Issue

GH-65

## Spec Packet

- Product: `specs/GH65/product.md`
- Tech: `specs/GH65/tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP65-T1` | web lane | GH59 | 实现 port hit target、connection drag state 和 compatible highlight。 | pointer cancel/drop/highlight 测试通过。 | `cd web && npm test -- app.test.tsx` |
| `SP65-T2` | server/graph lane | GH59 | 如需要,扩展 manual proposal 支持批量 ops 并保留单 op 兼容。 | replace edge 原子 preview,旧单 op 测试仍通过。 | `cargo test -p helixflow-server manual_proposal && cargo test -p helixflow-graph` |
| `SP65-T3` | web lane | `SP65-T1`,`SP65-T2` | 接入 add_edge、remove_edge、replace confirm UI。 | 连接/断开/替换产生正确 pending proposal。 | `cd web && npm test -- app.test.tsx` |

## 并行拆分

web drag helper 与 server batch op 可并行;最终接入由 web lane 完成。

## 验证

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP65-T4` | verification lane | `SP65-T1`-`SP65-T3` | 全仓测试和拖线手工冒烟。 | 兼容连线、拒绝不兼容、断线、restore 均通过。 | `cargo check --workspace && cargo test --workspace && cd web && npm test && npm run build` |

## Handoff Notes

- 后端 GraphService validation 是最终事实来源。
- 已占用 input 的替换必须是一个原子 proposal。
