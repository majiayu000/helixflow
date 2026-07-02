# Task Plan

## Linked Issue

GH-67

## Spec Packet

- Product: `specs/GH67/product.md`
- Tech: `specs/GH67/tech.md`

## 实现任务

- [ ] `SP67-T1` Owner: backend-agent. 重构 `crates/agent/src/service.rs` 为 bounded retry loop,保留 `max_attempts=1` 兼容路径。Done when: fake Codex invalid->valid 在第二轮成功。Verify: `cargo test -p helixflow-agent retry`
- [ ] `SP67-T2` Owner: backend-agent. 在 prompt stack 增加上一轮 validation error feedback layer。Done when: rendered prompt 包含 attempt/error/opIndex 摘要且不包含 secret。Verify: `cargo test -p helixflow-agent prompt_feedback`
- [ ] `SP67-T3` Owner: backend-agent. 每轮复用同一 proposal parse/preview validation,格式错误也回喂。Done when: invalid JSON 与 graph validation error 都能进入下一轮。Verify: `cargo test -p helixflow-agent validation_retry`
- [ ] `SP67-T4` Owner: backend-runtime. 将 attempt started/failed/retrying/validated/exhausted 写入 runtime log/status,并处理 cancel。Done when: runtime log 可见每轮错误,取消后不再启动下一轮。Verify: `cargo test -p helixflow-agent retry_status`

## 并行拆分

- prompt feedback helper 可先做。
- service retry loop、validation path、runtime status 修改共享 agent service,需串行整合。

## 验证

- [ ] `SP67-T5` Owner: coordinator. 全量验收多轮 agent。Done when: `cargo test -p helixflow-agent retry && cargo test -p helixflow-agent prompt_feedback && cargo test -p helixflow-agent validation_retry && python3 checks/check_workflow.py --repo . --spec-dir specs/GH67` 全部通过。Verify: `cargo test -p helixflow-agent retry && cargo test -p helixflow-agent prompt_feedback && cargo test -p helixflow-agent validation_retry && python3 checks/check_workflow.py --repo . --spec-dir specs/GH67`

## Handoff Notes

- 依赖 GH-59 统一 op/proposal validation 语义稳定后实现。
- 不切换直接 API/tool-calling。

