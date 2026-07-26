# Task Plan

## Linked Issue

GH-144

## Spec Packet

- Product: `specs/GH144/product.md`
- Tech: `specs/GH144/tech.md`

## 实现任务

- [x] `SP144-T1` server：`migration_routes.rs`（dry-run/apply）+ `CandidateKind::MigratedGraph` + 路由注册。Owner: server。Done when: 五条不变量对应的 route 测试全绿。Verify: `cargo test -p helixflow-server migration`
- [x] `SP144-T2` web：TopBar 迁移入口 + `migration-panel.tsx`（计数、节点原因列表、apply、刷新）。Owner: frontend。Done when: 面板测试 + tsc + 构建通过。Verify: `cd web && npx tsc --noEmit && npm test`
- [x] `SP144-T3` 回归：全量 workspace 与 web 测试加 SpecRail 校验。Owner: coordinator。Done when: 全部命令 exit 0 且输出来自当次会话。Verify: `cargo test --workspace && python3 checks/check_workflow.py --repo . --spec-dir specs/GH144`

## 验证

- [x] `SP144-T4` 交付前按各任务 Verify 全量复跑并附会话内输出。Owner: coordinator。Done when: T1–T3 的 Verify 全部通过。Verify: `cargo test --workspace`

## Handoff Notes

- 审计标记：label `v1→v2 migration` + source=manual + semantics_json 非 NULL；
  为 #145 提供"存量已迁移或明确隔离"的查询依据（versions 表可数）。
