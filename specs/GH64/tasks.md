# Task Plan

## Linked Issue

GH-64

## Spec Packet

- Product: `specs/GH64/product.md`
- Tech: `specs/GH64/tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP64-T1` | web lane | GH59 | 增加 Inspector schema control helper 和 typed local validation。 | string/number/integer/boolean/enum/unknown 类型测试通过。 | `cd web && npm test -- app.test.tsx` |
| `SP64-T2` | web lane | `SP64-T1` | 接入 set_param manual proposal submit、pending gate 和错误展示。 | 合法提交产生 pending proposal,已有 pending 时禁用/提示。 | `cd web && npm test -- app.test.tsx` |
| `SP64-T3` | server lane | none | 补充 manual `SetParam` 错误映射/测试。 | stale base、类型范围错误返回可读 4xx。 | `cargo test -p helixflow-server manual_set_param` |

## 并行拆分

web 控件和 server 错误测试可并行;最终由 web lane 接入 store action。

## 验证

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP64-T4` | verification lane | `SP64-T1`-`SP64-T3` | 全仓验证和手工编辑冒烟。 | 修改 prompt/seed/尺寸路径通过。 | `cargo check --workspace && cargo test --workspace && cd web && npm test && npm run build` |

## Handoff Notes

- 不绕过 proposal gate;只复用 manual proposal route。
- 参数 typed value 不要用字符串包裹 number/boolean。
