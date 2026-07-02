# Tech Spec

## Linked Issue

GH-65

## Product Spec

`specs/GH65/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Canvas pointer system | `web/src/components/graph-canvas.tsx`, `graph-canvas-node` | 已有节点拖拽、框选和 pointer capture | 端口拖拽应接入同一 pointer 生命周期 |
| Port metadata | `crates/registry/src/lib.rs`, `web/src/types.ts` | catalog 暴露 input/output port type 和 required | 前端兼容高亮要基于同一 PortType |
| Edge validation | `crates/graph/src/lib.rs` | 后端 validate_graph 检查 missing port、duplicate input、type mismatch | 后端仍是最终校验 |
| Manual proposal API | `crates/server/src/manual_proposal_routes.rs` | 已有 `AddEdge` / `RemoveEdge` 单 op | 需要支持替换场景的批量原子 op |

## 设计方案

前端新增 `connectionDrag` 状态,从 output port pointer down 开始记录 source node/port/type。拖拽中根据 catalog 计算兼容 input port 集合,渲染临时连线和 port highlight。pointer up 时若落在兼容 input 且无 pending proposal,提交 edge op。

未占用 input 使用现有 `AddEdge` manual op。已占用 input 的替换需要原子删除旧边并添加新边;如果 GH59 已提供批量 op endpoint,前端使用批量 manual proposal;否则本 issue 需要把 manual proposal request 从单 `op` 扩展为 `ops` 并保留单 op 兼容。server 对批量 ops 调用同一个 `GraphService::preview_proposal`,保证 replace 是一个 pending proposal 和一个版本。

断线入口可先支持边 context menu 或选中边后的 Disconnect 按钮,提交 `RemoveEdge` op。后续再做边直接拖离输入端口。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1、P3 | connection drag/highlight helper | web 测试:兼容/不兼容端口状态 |
| P2、P5 | AddEdge/RemoveEdge manual proposal | server/web 测试:preview proposal 正确 |
| P4 | batch replace op | graph/server 测试:remove+add 原子 preview,duplicate input 不泄漏 |
| P6 | GraphService validation | server 测试:type mismatch 返回 4xx |

## 数据流

port pointer down -> connection drag state -> compatible port highlight -> pointer up target -> optional replace confirmation -> manual proposal add/remove/batch ops -> previewGraph -> pending proposal UI。

## 备选方案

- 只在前端修改 edge state:被否,绕过 version/proposal gate。
- 允许输入端多边:被否,违反现有 GraphService duplicate input 规则。

## 风险

- Security: 无新增外部数据面。
- Compatibility: 如果扩展 `ManualProposalRequest` 支持 `ops`,需保持现有单 `op` request 兼容。
- Performance: 大图高亮要预计算 compatible map,避免每次 pointer move 扫全图。
- Maintenance: PortType 映射必须复用 catalog,不要硬编码文本。

## 测试计划

- [ ] Frontend tests: drag state、highlight、compatible drop、不兼容 cancel、replace confirm。
- [ ] Server tests: batch remove+add edge,duplicate input 和 type mismatch。
- [ ] Manual verification: 拖线连接、断线、apply、restore。

## 回滚方案

隐藏端口拖拽和断线 UI 即可回到手动 proposal 表单;如扩展 batch API,保留单 op 兼容。
