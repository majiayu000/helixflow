# GH130 IntentPlan Fixtures（SP130-T0 固化）

六类意图场景的冻结基线，供 SP130-T3 的 `WorkflowCompiler` golden 测试使用。
schema 对应 `specs/GH130/tech.md` 第 4 节 `IntentPlan`；JSON 键使用 camelCase（P15）。

| 文件 | 场景 | 期望结果 |
| --- | --- | --- |
| `intent-nano-banana-seedance.json` | 点名双模型串行链（20.1） | compiled，单主链 |
| `intent-gpt-image-seedance.json` | 首阶段模型替换（20.2） | compiled，拓扑与上一条一致 |
| `intent-serial-chain.json` | 三阶段显式串行 | compiled，不得 fan-out（P6） |
| `intent-explicit-parallel.json` | 明确并行双分支（20.3） | compiled，恰好两分支 |
| `intent-missing-image-input.json` | image_to_video 缺 image 输入 | clarify_first / REQUIRED_INPUT_MISSING（P7） |
| `intent-model-mismatch.json` | pinned 模型无对应 binding | error / BINDING_NOT_FOUND（P4、P8） |

`expected` 块由 `crates/compiler/src/compile_tests.rs` 的 golden 测试逐条断言
（SP130-T3）。SP130-T4 起 provider 静默默认模型已删除，基线测试翻转为
fail-closed 断言：

- `crates/gateway/src/atlas/tests.rs`：`gh130_failclosed_invokes_require_resolved_operation`、`gh130_resolved_operation_drives_request_model`
- `crates/gateway/src/fal/tests.rs`：`gh130_failclosed_image_without_resolved_operation_errors`、`gh130_resolved_operation_drives_submit_path`
- `crates/registry/src/lib.rs`：`gh130_baseline_params_model_is_rejected_as_unknown`（durable graph 仍拒绝 `params.model`）
- `crates/agent/src/tests.rs`：`gh130_baseline_proposal_cannot_pin_model_via_params`
- `crates/run/src/resolved_tests.rs`：运行期解析与 PINNED_MODEL_MISMATCH 校验
