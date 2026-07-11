# Task Plan

## Linked Issue

GH-108 (#108)

## Tasks

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP108-T1 | backend | none | 增加 fallible retry-limit parser/env reader | missing 与 invalid 明确分离 | focused run-policy tests |
| SP108-T2 | backend | T1 | self-heal 传播配置错误 | 非法配置在 retry 写入前失败 | Rust self-heal regression |
| SP108-T3 | docs/qa | T1-T2 | 更新配置说明并全量验证 | invalid 行为有文档且所有 gate 通过 | cargo check/test + SpecRail |

## PR Ownership

- `crates/run/src/run_policy.rs`
- `crates/run/src/self_heal.rs`
- retry configuration documentation and focused tests

## Verification

- `cargo check --workspace`
- `cargo test --workspace`
- `python3 checks/check_workflow.py --repo . --spec-dir specs/GH108`
- `python3 checks/check_workflow.py --repo . --all-specs`
