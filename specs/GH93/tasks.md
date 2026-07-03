# Task Plan

## Linked Issue

GH-93

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

| ID | Owner | Depends on | Task | Done when | Verification |
| --- | --- | --- | --- | --- | --- |
| SP93-T0 | maintainer | none | 完成 spec review 和 implementation readiness gate | `GH93` 的 product/tech/tasks 被维护者接受，并推进到 `ready_to_implement` | `python3 checks/route_gate.py --repo . --route implement --issue 93 --state ready_to_implement --json` |
| SP93-T1 | backend | SP93-T0 | 添加 graph-only workspace bootstrap compatibility tests | 只有旧 graph 数据的 workspace 可作为有效 canvas 打开 | `cargo test --workspace` |
| SP93-T2 | backend | SP93-T1 | 添加 snapshot seq replay tests | store seq ahead of snapshot seq 会 replay missing ops，snapshot ahead 会明确报错 | `cargo test --workspace` |
| SP93-T3 | frontend/backend | SP93-T1 | 添加 canvas edits reload persistence tests | add/move/connect/comment 的 durable state reload 后仍存在 | `cargo test --workspace && cd web && npm test` |
| SP93-T4 | qa | SP93-T3 | 添加 deterministic full-story regression 或 manual script | open/add/move/connect/comment/run/approve/artifact/reload 流程可重复执行 | `cargo test --workspace && cd web && npm test && npm run build` |
| SP93-T5 | docs | SP93-T4 | 行为稳定后更新 docs | docs 只描述已验证行为，不承诺 provider/identity 非范围能力 | `python3 checks/check_workflow.py --repo . --all-specs` |

## 并行拆分

- Backend compatibility lane owns store/server tests and migration fixtures.
- Web regression lane owns web tests and optional deterministic script.
- Docs lane owns docs updates after behavior is proven.

## 验证

- `cargo test --workspace`
- `cd web && npm test`
- `cd web && npm run build`
- `python3 checks/check_workflow.py --repo .`
- `python3 checks/check_workflow.py --repo . --all-specs`

## Handoff Notes

This issue should run after or alongside GH88-GH92 verification. Do not make the
manual/E2E path depend on live provider credentials.
