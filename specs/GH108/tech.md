# Tech Spec

## Linked Issue

GH-108 (#108)

## Current Gap

`crates/run/src/run_policy.rs::max_run_retries()` 使用 `ok -> parse.ok -> unwrap_or(1)`，把 missing 与 invalid 合并为同一结果。`self_heal::prepare_retry()` 因而无法区分默认值和错误配置。

## Design

1. 新增纯函数 `parse_max_run_retries(raw: Option<&str>) -> RunResult<u32>`。
2. `None` 返回 `1`；`Some` 只接受 Rust `u32` 十进制解析范围。
3. `max_run_retries() -> RunResult<u32>` 单独处理 `NotPresent` 和 `NotUnicode`。
4. `prepare_retry()` 使用 `max_run_retries()?`，在任何数据库写入前传播配置错误。
5. error 文案固定引用 `HELIXFLOW_RUN_MAX_RETRIES` 和 non-negative integer 约束。

## Tests

- parser default/valid/boundary tests。
- parser malformed/negative/overflow tests。
- 现有 self-heal tests 继续证明默认行为。
- Rust workspace check/test。

## Safety

- 不读取或记录 secret。
- 不对非法值 fallback；错误在 retry 派生写入前发生。
- 不修改 confirmation threshold 的独立契约。
