# Task Plan

## Linked Issue

GH-144

## Spec Packet

- Product: `specs/GH144/product.md`
- Tech: `specs/GH144/tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP144-T0` | graph | none | 将 legacy migrator 适配 GH145 embedded schema，增加 typed reason 与 workspace connector restriction。 | mapping 确定、topology 不变、跨 connector fail-closed。 | `cargo test -p helixflow-graph semantics` |
| `SP144-T1` | store | none | 0007 schema、assessment/audit、`source=migration`、operation replay 与 current+connector+pending CAS。 | upgrade 无损；workspace 删除级联；apply 原子；同 ID 重放；并发最多一个 current。 | `cargo test -p helixflow-store version_migration` |
| `SP144-T2` | server | `T0`,`T1` | version-bound dry-run/apply、canonical hash、replay-first、apply flag、candidate lifecycle/reconciliation。 | stale/disabled/unresolved 零 target；成功 source 不变；embedded-only 不重复迁移；migration orphan 可回收。 | `cargo test -p helixflow-server version_migration` |
| `SP144-T3` | writers | `T2` | 审计 layout、restore、ops、manual/agent proposal 的 embedded semantics 与 sidecar 派生。 | 所有 derived version 保持 canonical semantics；缺失时 fail-closed。 | `cargo test -p helixflow-server` |
| `SP144-T4` | frontend data | `T2` | 独立 Zod/API/Zustand migration state machine。 | stale dry-run abort；unknown 同 ID retry；late success 延迟 hydrate；确定错误不伪装 unknown。 | `cd web && npx tsc --noEmit && npm test -- --run` |
| `SP144-T5` | frontend UI | `T4` | version history migration panel、确认、connector/影响计数、dirty/no-current guard、node/top-level failure 与 accessibility。 | server applyEnabled、operation ID fallback、全部互斥状态与 aria 测试通过。 | `cd web && npm test -- --run && npm run build` |
| `SP144-T6` | coordinator | `T0`–`T5` | merge 最新 main，移除无门禁旧 endpoint/UI，执行 exact-head review、CI 与 PR gate。 | diff 与当前 embedded architecture/spec 一致；fresh checks green；无 actionable review finding。 | `cargo check --workspace && cargo test --workspace && python3 checks/check_workflow.py --repo . --all-specs` |

## 验证

本地交付命令：

```sh
cargo fmt --check
cargo check --workspace
cargo test --workspace
cd web && npx tsc --noEmit && npm test -- --run && npm run build
cd .. && python3 checks/check_workflow.py --repo . --spec-dir specs/GH144
python3 checks/check_workflow.py --repo . --all-specs
```

latest-head independent review、GitHub Actions 与 `pr_gate.py` 属于合并前门禁，不以
本地测试替代。

## Handoff Notes

- `WorkflowGraph.catalog_revision` + `GraphNode.semantics` 是 GH145 canonical truth。
- `versions.semantics_json` 仅为旧版本 fallback 与 derived index，不能作为迁移完成判定。
- 旧 `/api/workspaces/{id}/graph-migration/*` 与 TopBar panel 已移除，避免绕过 report、
  operation、connector 与 apply flag 门禁。
- apply 默认关闭；合并不等于开启生产灰度。
- #146 仍受两个明确 release 与稳定性证据 gate 约束，本任务不提前删除 legacy contract。
