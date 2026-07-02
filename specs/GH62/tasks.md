# Task Plan

## Linked Issue

GH-62

## Spec Packet

- Product: `specs/GH62/product.md`
- Tech: `specs/GH62/tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP62-T1` | store lane | none | 新增 `node_cache_entries` migration 和 Store 查询/upsert/校验辅助。 | cache entry CRUD、workspace 隔离和损坏 artifact miss 单测通过。 | `cargo test -p helixflow-store cache` |
| `SP62-T2` | run lane | `SP62-T1` | 在 RunService step 执行前计算 cache key,hit 时跳过 provider,miss 成功后刷新 cache。 | 末端/中游参数变更 invoke count、provider 分隔、force rerun 单测通过。 | `cargo test -p helixflow-run cache` |
| `SP62-T3` | server lane | `SP62-T2` | run 请求接收 `forceRerun`,workspace state 输出 step cached metadata。 | API schema、state payload 和二次 run 集成测试通过。 | `cargo test -p helixflow-server cache` |
| `SP62-T4` | web lane | `SP62-T3` | 增加 cached step 标记和 force rerun 控制。 | UI 可见 cached badge,force rerun 请求参数正确。 | `cd web && npm test -- app.test.tsx` |

## 并行拆分

store lane 与 web badge 设计可先行;run lane 依赖 store helper;server lane 依赖 run 请求/metadata shape。

## 验证

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP62-T5` | verification lane | `SP62-T1`-`SP62-T4` | 全仓测试和本地缓存冒烟。 | 修改末端和中游参数的手工路径与自动测试一致。 | `cargo check --workspace && cargo test --workspace && cd web && npm test && npm run build` |

## Handoff Notes

- provider id、upstream artifact hash、params 都必须进入 key。
- cache hit 不新增 actual provider cost,但必须创建本次 run 可见的 artifact/state。
