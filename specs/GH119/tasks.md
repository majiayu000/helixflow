# Task Plan

## Linked Issue

GH-119：<https://github.com/majiayu000/helixflow/issues/119>

## Spec Packet

- Product: [`product.md`](product.md)
- Tech: [`tech.md`](tech.md)

## 实现任务

- [x] `SP119-T1` — Owner: GH-119 artifact I/O lane。Dependencies: none。先添加路径 traversal/absolute/separator/symlink canary 与 opaque filename 安全测试。Done when: 旧实现会把至少一个 client-controlled component 带入路径或写出预期 boundary，失败日志保存。Verify: `cargo test -p helixflow-run artifact_path -- --nocapture`。
- [x] `SP119-T2` — Owner: GH-119 artifact I/O lane。Dependencies: `SP119-T1`。实现 lexical+canonical root enforcement、服务端 opaque filename、同目录 `.part` create-new 与原子发布/清理。Done when: P1–P3/P9 tests 全部通过且既有 inline artifact 回归通过。Verify: `cargo test -p helixflow-run artifact_path -- --nocapture`。
- [x] `SP119-T3` — Owner: GH-119 artifact I/O lane。Dependencies: none。先添加 URL/IP forbidden range、每跳 redirect、timeout/redirect 配置、Content-Length/实际字节上限、MIME 与 partial cleanup 安全测试。Done when: 旧实现缺失对应 policy/helper，focused suite 在实现前稳定失败并保存日志。Verify: `cargo test -p helixflow-run artifact_remote -- --nocapture`。
- [x] `SP119-T4` — Owner: GH-119 artifact I/O lane。Dependencies: `SP119-T2`、`SP119-T3`。实现 restricted client、逐跳 DNS/address validation+pin、手动 redirect、总 timeout 与有界流式写入。Done when: P4–P7/P9/P10 tests 通过，安全错误不回显 URL/root。Verify: `cargo test -p helixflow-run artifact_remote -- --nocapture`。
- [x] `SP119-T5` — Owner: GH-119 artifact I/O lane。Dependencies: `SP119-T4`。把 response MIME 和下载完成后的 PR #117 media validator 接入成功门禁；保留合法 inline/HTTPS/media 路径。Done when: mismatch/invalid media 无 final artifact，合法 fixture 成功。Verify: `cargo test -p helixflow-run artifact -- --nocapture`; `cargo test -p helixflow-run invalid_media -- --nocapture`。
- [x] `SP119-T6` — Owner: GH-119 artifact I/O lane。Dependencies: `SP119-T1`–`SP119-T5`。运行 full verification，原始大日志写入 `artifacts/logs/gh119/`。Done when: focused、workspace check/test、GH119/base/all-specs 与 diff check 全部 exit 0。Verify: 见“验证”。
- [ ] `SP119-T7` — Owner: GH-119 artifact I/O lane。Dependencies: `SP119-T6`。提交、push 并创建 final-slice mixed implementation PR。Done when: 中文 PR 包含 `Fixes #119`、`pr_kind: mixed_impl`、auto-applied readiness 来源与 fresh test evidence；不自审、不合并。Verify: `gh pr view --json number,url,headRefName,body`。

## 并行拆分

本 issue 由单一可写 lane 串行完成。独占范围为 `specs/GH119/**`、
`crates/run/src/artifacts.rs`、新增 `crates/run/src/artifact_*.rs` 和必要的 `crates/run/src/lib.rs`；
只有 fresh compile 证明必要时才修改 Cargo 文件。禁止修改 `crates/server/**`、
`crates/store/**`、`web/**`、GH118/GH120、`.specrail/runtime/current.json` 与其他 lane 文件。

## 验证

- Reproduction: `artifacts/logs/gh119/repro-artifact-path.log`（同一旧代码 run 同时记录 path、
  symlink 与 remote loopback/redaction 三项失败）。
- Focused: `cargo test -p helixflow-run artifact_path -- --nocapture`、
  `cargo test -p helixflow-run artifact_remote -- --nocapture`、
  `cargo test -p helixflow-run artifact -- --nocapture`、
  `cargo test -p helixflow-run invalid_media -- --nocapture`。
- Build: `cargo check --workspace`。
- Full Rust: `cargo test --workspace`。
- Spec: `python3 checks/check_workflow.py --repo . --spec-dir specs/GH119`。
- Workflow: `python3 checks/check_workflow.py --repo .` 与 `--all-specs`。
- Git: `git diff --check` 与 scoped status/diff。

## Handoff Notes

- Selected locale: `zh-CN`；stable IDs、commands、paths、JSON keys 保持 English。
- 仓库没有 duplicate evidence script；已手工搜索 remote branch、全部 PR title/body 与 issue，
  无 #119 重复。PR #117 已合并且明确把 SSRF、redirect、资源上限、node id path boundary 排除在
  scope 外；不伪造脚本证据。
- live `write_spec` issue evidence 因 issue 初始无 readiness label 返回 `needs_human`。本次 parent
  coordinator 基于用户明确的 `implx auto`/`auto_drain` 授权，显式提供 `ready_to_spec` 状态；对应
  local gate 返回 `allowed`。spec checks 通过后，本 lane 将按同一授权给 #119 添加
  `ready_to_implement`，刷新 issue evidence 并记录 label mutation/source。
- 剩余 human gates：human final review、security decision、merge authorization 与 merge。本 lane
  不自审、不提供 final approval、不合并。
