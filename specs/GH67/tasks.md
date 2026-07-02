# Task Plan

## Linked Issue

GH-67

## Spec Packet

- Product: `specs/GH67/product.md`
- Tech: `specs/GH67/tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP67-T1` | agent lane | GH59 | 为 AgentService 增加 proposal retry loop、max rounds 配置和 validation feedback 类型。 | fake runtime 坏->好、连续失败 exhausted 单测通过。 | `cargo test -p helixflow-agent retry` |
| `SP67-T2` | agent lane | `SP67-T1` | 增加 retry prompt/turn 构造,清理或版本化每轮 output。 | retry turn 含结构化错误且不泄露路径/secrets。 | `cargo test -p helixflow-agent prompt_stack retry` |
| `SP67-T3` | server/web lane | `SP67-T1` | 确认每轮 agent.status/log 投影到 UI,最终只展示合法 pending proposal。 | 轮次状态、失败错误和成功 proposal 测试通过。 | `cargo test -p helixflow-server agent && cd web && npm test -- app.test.tsx` |

## 并行拆分

agent loop 与 UI log 展示可部分并行;retry prompt 依赖 feedback shape。

## 验证

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP67-T4` | verification lane | `SP67-T1`-`SP67-T3` | 全仓测试和 fake bad proposal 冒烟。 | 首轮坏图 ≤3 轮自愈;超限失败可读。 | `cargo check --workspace && cargo test --workspace && cd web && npm test && npm run build` |

## Handoff Notes

- Chat/ReplyJson 模式保持单轮。
- 不切换到直接 LLM API;继续 Codex CLI。
