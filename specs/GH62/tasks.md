# Task Plan

## Linked Issue

GH-62

## Spec Packet

- Product: `specs/GH62/product.md`
- Tech: `specs/GH62/tech.md`

## 实现任务

- [ ] `SP62-T1` Owner: backend-store. 新增 workspace-local node cache schema 与 store helper。Done when: 可按 `(workspace_id,node_id,cache_key)` 查询/写入 artifact refs,并验证 artifact 存在。Verify: `cargo test -p helixflow-store cache`
- [ ] `SP62-T2` Owner: backend-run. 在 RunService 增加 cache key builder、cache read gate、miss 写入和 force rerun flag。Done when: 末端改参上游不调用 provider,中游改参只重跑下游。Verify: `cargo test -p helixflow-run cache`
- [ ] `SP62-T3` Owner: backend-server. 将 force rerun 暴露到 run request / workspace state payload,并标记 cached step。Done when: API payload 能区分 cached 与新执行 step。Verify: `cargo test -p helixflow-server cache`
- [ ] `SP62-T4` Owner: frontend. 在 run step/UI 中展示 cached badge,保留 artifact preview。Done when: cached step 可读且不显示为 running。Verify: `cd web && npm test -- app.test.tsx`

## 并行拆分

- store lane 先完成 schema/helper。
- run lane 依赖 store helper 接口。
- frontend lane 可基于 payload type 草案先改展示,最终联调在 coordinator。

## 验证

- [ ] `SP62-T5` Owner: coordinator. 全量验收 cache 行为。Done when: `cargo test -p helixflow-store cache && cargo test -p helixflow-run cache && cargo test -p helixflow-server cache && (cd web && npm test -- app.test.tsx) && python3 checks/check_workflow.py --repo . --spec-dir specs/GH62` 全部通过。Verify: `cargo test -p helixflow-store cache && cargo test -p helixflow-run cache && cargo test -p helixflow-server cache && (cd web && npm test -- app.test.tsx) && python3 checks/check_workflow.py --repo . --spec-dir specs/GH62`

## Handoff Notes

- 不跨 workspace 共享 cache。
- force rerun 只跳过 read,成功后仍可写 cache。

