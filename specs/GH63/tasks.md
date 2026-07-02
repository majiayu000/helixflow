# Task Plan

## Linked Issue

GH-63

## Spec Packet

- Product: `specs/GH63/product.md`
- Tech: `specs/GH63/tech.md`

## 实现任务

- [ ] `SP63-T1` Owner: backend-run. 为 compiled plan 构建 dependency graph、ready queue 和 bounded worker scheduler。Done when: max concurrency 1 等价串行,max 2 菱形图 B/C 并发。Verify: `cargo test -p helixflow-run scheduler`
- [ ] `SP63-T2` Owner: backend-run. 实现 failure propagation、downstream skipped 和 terminal run state 收敛。Done when: 一个分支失败时依赖下游不执行,run failed 可对账。Verify: `cargo test -p helixflow-run failure`
- [ ] `SP63-T3` Owner: backend-run. 将 interrupt token fanout 到所有 in-flight step,处理 provider 迟到结果。Done when: 中断慢 provider 双分支后无 hanging step。Verify: `cargo test -p helixflow-run interrupt`
- [ ] `SP63-T4` Owner: backend-server. 暴露 `max_node_concurrency` 配置并保持 workspace state/event payload 兼容。Done when: config 默认值生效,前端无需依赖全局事件顺序。Verify: `cargo test -p helixflow-server concurrency`

## 并行拆分

- scheduler/failure/interrupt 都在 run executor 内,应串行完成。
- server config lane 可在 scheduler API 稳定后并行接入。

## 验证

- [ ] `SP63-T5` Owner: coordinator. 全量并发验收。Done when: `cargo test -p helixflow-run scheduler && cargo test -p helixflow-run interrupt && cargo test -p helixflow-server concurrency && python3 checks/check_workflow.py --repo . --spec-dir specs/GH63` 通过。Verify: `cargo test -p helixflow-run scheduler && cargo test -p helixflow-run interrupt && cargo test -p helixflow-server concurrency && python3 checks/check_workflow.py --repo . --spec-dir specs/GH63`

## Handoff Notes

- 依赖 GH-58 后台 run/interrupt 和 GH-62 cache 语义稳定后实现。
- 不做跨 run 或多机调度。

