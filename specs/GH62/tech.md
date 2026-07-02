# Tech Spec

## Linked Issue

GH-62

## Product Spec

`specs/GH62/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Run loop | `crates/run/src/lib.rs` | `execute_created_run` 对 `plan.steps` 顺序循环执行,每个 provider step 都调用 provider | cache hit 必须在 step 执行前决策并跳过 provider |
| Artifacts | `crates/store/src/run_records.rs`, `artifacts` table | artifact 按 run/step 持久化,无跨 run node cache 记录 | 需要新增 cache table 并把 cache artifact 复制/关联到本次 run |
| Graph metadata | `crates/graph/src/lib.rs`, `crates/server/src/graph_files.rs` | version 有 `graph_hash`;节点 params 和 edges 可序列化 | cache key 需要稳定 canonical hash,不能只用整图 hash |
| Frontend state | `crates/server/src/workspace_state.rs`, `web/src/types.ts` | run step 有 state/progress/provider,无 cached 标记 | UI 需要知道 step 是 cache hit |

## 设计方案

新增 store migration `node_cache_entries`:字段包含 workspace_id、provider、node_type、node_id、cache_key、input_hash_json、artifact_ids_json、created_at、last_hit_at。cache key 由 canonical JSON 计算:node_type、provider、capability、params、输入端口值、上游 artifact content hash 和 schema version。cache scope 限定 workspace,不跨 workspace 共享。

RunService 执行每个 step 前先解析其上游输入内容 hash。若请求没有 `force_rerun` 且 cache entry 存在、artifact 文件仍存在且 hash 校验通过,则当前 run step 直接写为 succeeded,run event 标记 `{"cached":true}`,并为本次 run 创建 artifact 记录指向已持久化内容或安全复制出的 content path。provider 不被调用,actual cost 不新增。若 miss 或缓存损坏,按现有执行路径运行;成功后写/刷新 cache entry。

force rerun 由 run 请求携带布尔值,默认 false。前端在 Run 按钮附近提供可发现的强制重跑控制,发送到 queue/confirm 路径;agent/sweep 路径默认不强制,除非后续 issue 扩展。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1、P2 | cache key 计算 + RunService 前置查询 | run 单测:修改末端/中游参数时 invoke count 符合预期 |
| P3 | provider 纳入 cache key | run 单测:同参数不同 provider 不命中 |
| P4、P5 | run step/artifact 投影 | server/web 测试:cached step 显示且 outputs 可预览 |
| P6 | request force flag | server/run 测试:force rerun 跳过 cache 并刷新 entry |
| P7 | cache 校验 | store/run 测试:artifact 缺失或 hash 不匹配按 miss 执行 |

## 数据流

queue/confirm 请求 -> RunService compile plan -> step ready -> compute cache key -> cache hit 写本次 step/artifact/event -> cache miss 调 provider -> 成功 artifact 落盘 -> upsert cache -> workspace state 返回 cached metadata。

## 备选方案

- 只按 node params 建 key:被否,忽略上游 artifact 会错误复用脏输入。
- 直接复用旧 artifact row 而不创建本次 run 关联:被否,本次 run 刷新和 outputs 历史会缺失。

## 风险

- Security: cache entry 不得保存原始 provider secret 或 signed URL。
- Compatibility: 需要 migration;旧数据不回填,首次 run 仍全量执行。
- Performance: canonical hash 不能读超大 artifact 到内存;需要流式 hash 或复用 artifact sha。
- Maintenance: cache invalidation 必须集中在 RunService,避免 UI 自行判断。

## 测试计划

- [ ] Unit tests: cache key canonicalization、provider 分隔、损坏 cache miss、force rerun。
- [ ] Integration tests: 末端/中游参数变更的 invoke count、cached step workspace state。
- [ ] Frontend tests: cached badge 和 force rerun 控制。

## 回滚方案

代码 revert 后忽略 `node_cache_entries`;保留表不影响运行。若需要彻底回滚,后续 migration 可 drop cache 表。
