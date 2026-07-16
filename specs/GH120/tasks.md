# Task Plan

## Linked Issue

GH-120

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done-when | Verify |
| --- | --- | --- | --- | --- | --- |
| SP120-T1 | GH120 store worker | none | 新增 comments state/operation migration 与集中 Store API | transaction 覆盖 CAS、replay、stale rollback；SQL 全部参数化 | `cargo test -p helixflow-store canvas_comment` |
| SP120-T2 | GH120 server worker | SP120-T1 | 接线 request `operationId`、SQLite truth、显式 legacy import 与 409/currentSeq | 不再用 JSON RMW；兼容 add/patch/resolve/delete | `cargo test -p helixflow-server canvas_collaboration` |
| SP120-T3 | GH120 server worker | SP120-T2 | 完成 40 路 same-base 并发/刷新重试和幂等测试 | 无 500、最终 40、sequence +40、retry 不重复 | `cargo test -p helixflow-server concurrent_same_base_comment_ops -- --nocapture` |
| SP120-T4 | GH120 worker | SP120-T1,T2,T3 | 完成 workspace/spec/diff 验证并保存日志 | 全部命令 fresh pass，diff 无越权文件 | `cargo check --workspace && cargo test --workspace` |
| SP120-T5 | GH120 worker | SP120-T4 | commit、push、创建 final `mixed_impl` PR | 中文 PR 含 `Fixes #120`，不 merge、不自审 | `gh pr view --json number,url,headRefName,baseRefName` |

## 并行拆分

本 issue 由一个可写 lane 串行完成，避免 server 与 Store contract 漂移。独占范围为
`specs/GH120/**`、`crates/server/src/canvas_collaboration.rs`、comments 专用 Store API、
migration 与对应 Rust tests。`crates/store/src/lib.rs` 只允许最小 comments module 声明/导出，
不得扩大到 graph consistency；不修改 `crates/run/**`、Web、graph/layout/proposal/version 文件。

## 验证

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| SP120-T6 | GH120 worker | SP120-T1..T5 | 保存 red proof，并执行 focused、workspace、SpecRail 与 diff 验证 | 旧实现失败证据可审计；新实现所有 fresh checks 通过 | `cargo test -p helixflow-store canvas_comment`; `cargo test -p helixflow-server canvas_collaboration`; `cargo test -p helixflow-server concurrent_same_base_comment_ops -- --nocapture`; `cargo check --workspace`; `cargo test --workspace`; `python3 checks/check_workflow.py --repo . --spec-dir specs/GH120`; `python3 checks/check_workflow.py --repo . --all-specs`; `git diff --check` |

## Handoff Notes

- Red proof 位于 `artifacts/logs/gh120/red-40-same-base-reproduction.log`：40 个请求中
  39 个返回成功，但只持久化 9 条且 `seq=9`。
- 用户以 implx auto 明确授权：spec checks 通过后为 #120 添加 `ready_to_implement`；仍禁止
  final approval、merge、force push 和自审。
- API boundary 使用 `operationId`、`baseSeq`、`currentSeq`；Rust 内部保持 snake_case。
- `operationId` 缺失是兼容路径，不宣称跨网络重试幂等；显式稳定 ID 才具备该保证。
