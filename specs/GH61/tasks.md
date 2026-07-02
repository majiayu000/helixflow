# Task Plan

## Linked Issue

GH-61

## Spec Packet

- Product: `specs/GH61/product.md`
- Tech: `specs/GH61/tech.md`

## 实现任务

- [ ] `SP61-T1` Owner: backend-provider. 实现 `fal` provider registration、`FAL_KEY` availability、`image_generate` invoke 与 static estimate。Done when: 无 key 显示 unavailable,有 key/mock key 时 provider descriptor 与 capability 正确。Verify: `cargo test -p helixflow-provider-gateway`
- [ ] `SP61-T2` Owner: backend-server. 将 fal provider 接入 workspace provider snapshot、run provider resolution 和 artifact persistence。Done when: 同一 graph 只切 selected provider 即可生成 fal provider request,且 artifact 走 safe content URL。Verify: `cargo test -p helixflow-server fal`
- [ ] `SP61-T3` Owner: backend-tests. 增加 mock fal HTTP e2e,覆盖 success、401/403、timeout、malformed result、non-image result 和 secret redaction。Done when: run failed/succeeded 状态与错误体符合 product invariants。Verify: `cargo test -p helixflow-server fal`
- [ ] `SP61-T4` Owner: docs. 更新部署配置说明,记录 `FAL_KEY`、首批模型范围和无 key 行为。Done when: 文档说明与 provider unavailable reason 一致。Verify: `rg "FAL_KEY" README.md docs crates`

## 并行拆分

- provider lane 独占 provider gateway/fal provider 文件。
- server lane 独占 server provider wiring、workspace state、artifact 相关测试。
- docs lane 只改 README/docs。

## 验证

- [ ] `SP61-T5` Owner: coordinator. 汇总验证。Done when: `cargo test -p helixflow-provider-gateway && cargo test -p helixflow-server fal && python3 checks/check_workflow.py --repo . --spec-dir specs/GH61` 全部通过。Verify: `cargo test -p helixflow-provider-gateway && cargo test -p helixflow-server fal && python3 checks/check_workflow.py --repo . --spec-dir specs/GH61`

## Handoff Notes

- 依赖 GH-57 provider framework/artifact persistence 合并并进入 `ready_to_implement` 后再实现。
- 不得硬编码 fal.ai key;所有外部 HTTP 测试使用 mock server。

