# Task Plan

## Linked Issue

GH-61

## Spec Packet

- Product: `specs/GH61/product.md`
- Tech: `specs/GH61/tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP61-T1` | gateway lane | GH57 provider registry | 新增 `FalProvider`,实现 env 装配、catalog summary、`image_generate` invoke/estimate 和脱敏错误映射。 | fake fal.ai 成功/401/429/unsupported capability 单测通过。 | `cargo test -p helixflow-gateway fal` |
| `SP61-T2` | server lane | `SP61-T1` | 把 `fal` 注册进 provider registry/workspace state,选择 unavailable `fal` 时拒绝 run。 | 有/无 `FAL_KEY` 两种 workspace state 和运行拒绝测试通过。 | `cargo test -p helixflow-server provider` |
| `SP61-T3` | run lane | `SP61-T1` | 确认 fal.ai output URL/bytes 走 artifact content 契约落盘,不暴露上游 URL。 | artifact preview/download 使用本地 content URL,`storage_uri` 非 http(s)。 | `cargo test -p helixflow-run artifact && cargo test -p helixflow-server artifact` |

## 并行拆分

gateway lane 先冻结 `FalProvider` 契约;server/run lane 在 provider payload shape 稳定后串行接线。

## 验证

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP61-T4` | verification lane | `SP61-T1`,`SP61-T2`,`SP61-T3` | 跑全仓检查和手工 fal.ai 冒烟。 | 本地 fake 测试全绿;真实 `FAL_KEY` 冒烟记录在 PR。 | `cargo check --workspace && cargo test --workspace && cd web && npm test && npm run build` |

## Handoff Notes

- 依赖 GH-57 实现的 provider registry、workspace provider selection 和 artifact content 端点。
- 不做 fal.ai 全量模型目录,先固定一个文生图模型并把 mapping 集中在 gateway。
