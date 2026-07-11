# Product Spec

## Linked Issue

GH-108 (#108)

## Problem

`HELIXFLOW_RUN_MAX_RETRIES` 当前在缺失、格式错误和非 UTF-8 三种情况下都会静默回退为 `1`。运维配置拼写错误因此会悄悄启用一次自动重试，与成本和审计预期不一致。

## Goals

- 环境变量缺失时保持默认 `1`。
- 合法非负 `u32` 值按配置生效，`0` 继续表示禁用自动重试。
- 已配置但格式错误、负数、越界或非 UTF-8 时返回明确 `InvalidConfiguration`。
- self-heal 在读取到非法配置时传播错误，不创建派生 retry run。

## Non-goals

- 不改变 retry 算法、成本确认阈值或 run event schema。
- 不新增任意的产品级最大重试次数；合法范围保持 `u32`。

## Acceptance Criteria

- AC1: unset -> `1`; `0`, `1`, `u32::MAX` 解析成功。
- AC2: 空串、负数、浮点、文本和超出 `u32` 的值显式失败。
- AC3: self-heal 调用链使用 fallible retry policy，非法配置不会静默进入默认分支。
- AC4: Rust workspace check/test 与 SpecRail checks 通过。

## Release Note

Invalid `HELIXFLOW_RUN_MAX_RETRIES` values now fail explicitly instead of silently enabling the default retry count.
