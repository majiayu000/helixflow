# Task Plan

## Linked Issue

GH-115：<https://github.com/majiayu000/helixflow/issues/115>

## Spec Packet

- Product: [`product.md`](product.md)
- Tech: [`tech.md`](tech.md)

## 实现任务

- [x] `SP115-T1` — Owner: GH-115 provider lane。Dependencies: none。先添加 registry 配置矩阵测试，稳定证明缺失 provider 会隐式得到 enabled mock、只设置 provider 时 mock 仍可执行。Done when: 测试在旧实现上按预期失败并保存原始日志。Verify: `cargo test -p helixflow-gateway registry -- --nocapture`。
- [x] `SP115-T2` — Owner: GH-115 provider lane。Dependencies: `SP115-T1`。实现 `unconfigured` fail-closed default 与 `HELIXFLOW_ENABLE_MOCK_PROVIDER` 显式开关，保持显式构造器用于测试。Done when: P1/P2 配置矩阵全部通过且 catalog 不包含隐式 enabled mock。Verify: `cargo test -p helixflow-gateway`。
- [x] `SP115-T3` — Owner: GH-115 provider lane。Dependencies: none。先添加无效 PNG/文本 MP4 persistence 与 executor 状态测试。Done when: 旧实现会写入无效文件或产生 succeeded，失败输出可稳定复现并保存日志。Verify: `cargo test -p helixflow-run invalid_media -- --nocapture`。
- [x] `SP115-T4` — Owner: GH-115 provider lane。Dependencies: `SP115-T3`。实现 PNG/MP4 写入前 validator，保证 inline/remote 共用，错误安全且无 DB artifact。Done when: invalid media 被拒，step/run failed、无 `run.succeeded`。Verify: `cargo test -p helixflow-run invalid_media -- --nocapture`。
- [x] `SP115-T5` — Owner: GH-115 provider lane。Dependencies: `SP115-T4`。把 mock image/video fixtures 改成可通过同一 validator 的确定性 synthetic media，并强化 catalog non-production 文案。Done when: 既有 mock run 回归成功且 P3/P7 断言通过。Verify: `cargo test -p helixflow-gateway`; `cargo test -p helixflow-run manual_run_persists_steps_events_and_artifacts`。
- [x] `SP115-T6` — Owner: GH-115 provider lane。Dependencies: `SP115-T2`。更新 server workspace/provider tests，证明生产默认 unavailable、显式 test mock 注入仍可用。Done when: server focused tests 通过，不修改 `web/**`。Verify: `cargo test -p helixflow-server provider`; `cargo test -p helixflow-server workspace_state`。
- [x] `SP115-T7` — Owner: GH-115 provider lane。Dependencies: `SP115-T1`–`SP115-T6`。运行完整验证并把原始日志写入 `artifacts/logs/gh115/`。Done when: focused tests、workspace check/test、SpecRail spec/all-spec gates 全部为 0 exit。Verify: 见“验证”。
- [ ] `SP115-T8` — Owner: GH-115 provider lane。Dependencies: `SP115-T7`。提交、push 并创建 final-slice mixed implementation PR。Done when: PR 中文正文含 `Fixes #115`、`pr_kind: mixed_impl`、auto-applied label 来源、测试证据；不合并、不自审。Verify: `gh pr view --json number,url,headRefName,body`。

## 并行拆分

本 issue 由单一可写 lane 串行完成，避免 registry、mock fixture 与 run validator 之间的共享
文件冲突。父 coordinator 与其他 lanes 不得修改 `specs/GH115/**`、本任务涉及的 Rust
provider/run/server 文件。当前 lane 禁止修改 `web/**`、`specs/GH114/**`、根 checkpoint
及用户主工作区脏文件。

## 验证

- Reproduction logs: `artifacts/logs/gh115/repro-registry.log`, `artifacts/logs/gh115/repro-invalid-media.log`。
- Focused: gateway registry/mock、run artifact validator/executor、server provider/workspace tests。
- Build: `cargo check --workspace`。
- Full Rust: `cargo test --workspace`。
- Spec: `python3 checks/check_workflow.py --repo . --spec-dir specs/GH115`。
- All specs: `python3 checks/check_workflow.py --repo . --all-specs`。
- Git/PR: clean scoped diff、remote branch、final-slice PR evidence。

## Handoff Notes

- Selected locale: `zh-CN`；stable IDs、env、commands、paths 保持 English。
- 远端 issue #115 初始无 readiness label；`write_spec` evidence gate 返回
  `needs_human`。本次 `implx auto_drain` 明确授权先完成 spec，并在 spec gates 通过后自动添加
  `ready_to_implement`；label mutation 与来源需记录在 lane/PR 日志。
- 仓库没有 `checks/github_duplicate_evidence.py`。已手工执行 open PR 与 remote branch 搜索，
  不伪造脚本输出；证据记录在 `artifacts/logs/gh115/lane.md`。
- Mock 的 backend catalog 已有 `Mock (local test)` / `local_test` 字段；本 lane 会强化 message，
  但遵守 ownership 不修改前端。若产品要求额外 UI badge，应另开 frontend slice。
- 剩余 human gates：human final review、merge authorization、merge；本 lane 不自审、不合并。
