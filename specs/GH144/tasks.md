# Task Plan

## Linked Issue

GH-144

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP144-T0` | graph lane | none | 固化 graph migration typed contract：为 unresolved/failed 分支增加稳定 `MigrationReasonCode`，生成稳定 node 顺序与候选顺序，并保持 `migrate_v1_to_v2` 不从 title/provider default 推断。文件所有权：`crates/graph/src/graph_v2.rs`、必要的新 migration module、`graph_v2_tests.rs`。 | 同一 graph/catalog/migration version 的 report 逐字节稳定；unknown node、model missing/ambiguous、capability mismatch、binding/default missing 均返回唯一稳定 code；现有 topology/幂等测试不削弱。 | `cargo test -p helixflow-graph` |
| `SP144-T1` | store lane | none | 增加 migration audit 与原子 store contract：下一条 SQL migration、`version_migration_records.rs`、`VersionSource::Migration`、operation fingerprint/replay/conflict/current CAS。文件所有权：`crates/store/migrations/*version_migration*.sql`、`crates/store/src/version_migration_records.rs`、相关 store tests、`crates/store/src/lib.rs` 的最小 module/export/enum 接线。 | target version、current pointer 与 audit 在同一 transaction 提交；source version 不变；相同 operation replay 返回同一 target，不同 payload 冲突；并发不同 operation 最多一个成功；fault injection 无半提交。 | `cargo test -p helixflow-store` |
| `SP144-T2` | server dry-run lane | `SP144-T0` | 实现 migration DTO、canonical `reportHash` 与 dry-run route；验证 workspace/current/version、graph path/hash/JSON、现有 v2 semantics，并将 typed graph report 转为 secret-free API。文件所有权：新 `crates/server/src/version_migration_routes.rs` 的 read/report 部分、独立 tests、`main.rs` route/module 最小接线。 | dry-run 零 database/file/current 写入；migratable、needs_resolution、already_migrated、failed 全分支可测；workspace/version mismatch、corrupt graph 与 invalid semantics fail-closed；route 继承 auth middleware。 | `cargo test -p helixflow-server version_migration` |
| `SP144-T3` | server apply lane | `SP144-T0`, `SP144-T1`, `SP144-T2` | 实现 apply precondition 重算、`operationId` 幂等、`CandidateKind::Migration` publish/store/mark-or-cleanup 协议和 `HELIXFLOW_V1_MIGRATION_APPLY` gate。文件所有权：`version_migration_routes.rs` 的 apply 部分、`version_file_consistency.rs` 的最小 candidate kind 扩展、server apply/concurrency/fault tests。 | 篡改或 stale report 返回 conflict；unresolved/disabled 不写入；成功创建一个 v2 current version且原 v1 不变；store/publish/cleanup failure 全量暴露；same/different operation 并发满足 spec。 | `cargo test -p helixflow-server version_migration && cargo test -p helixflow-store version_migration` |
| `SP144-T4` | frontend data lane | `SP144-T2`, `SP144-T3` | 增加独立 Zod DTO、API client 与 Zustand migration slice，不把业务逻辑塞入已接近 U-16 上限的 `api.ts`/`store.ts`/`types.ts`。文件所有权：`web/src/version-migration-types.ts`、`api-version-migration.ts`、`store-version-migration.ts`、对应 tests，以及 `store-types.ts`/`store.ts` 的最小组合接线。 | 所有 server response 先经 Zod 校验；workspace generation/version 变化 abort 并丢弃 stale response；未知 apply 结果保留 operation ID 并可安全重试；hydrate 后以 server truth 为准。 | `cd web && npx tsc --noEmit && npm test -- version-migration` |
| `SP144-T5` | frontend UI lane | `SP144-T4` | 在 version history 邻近增加独立 migration panel，覆盖 report、node resolution、explicit confirm、apply flag、conflict/failure/success 与 accessibility。文件所有权：`web/src/components/version-migration-panel.tsx`、组件 CSS/测试、`run-panels.tsx` 的最小挂载。 | current version 才显示入口；每个 unresolved node 可定位；loading/disabled/error/conflict/success 不互相伪装；键盘操作、文本 label 与 `aria-live` 测试通过；workspace 切换不显示旧报告。 | `cd web && npx tsc --noEmit && npm test -- version-migration-panel` |
| `SP144-T6` | coordinator | `SP144-T0`–`SP144-T5` | 完成跨层验收、rollout evidence 与 handoff：运行全量 Rust/TS/SpecRail 检查，核对 P1–P17、secret-free contract、source immutability、candidate reconciliation，并记录 #145/#146 所需 migration coverage 字段。文件所有权：仅验证证据、PR body/handoff；发现问题回到所属 lane 修 production code，不弱化测试。 | 所有 deterministic checks fresh green；PR diff 与 GH144 specs/tasks 一致；无未解决 actionable review thread；明确 dry-run-first、apply flag 默认关闭及 rollback 证据；不宣称 merge authorization。 | `cargo check --workspace && cargo test --workspace && cd web && npx tsc --noEmit && npm test && cd .. && python3 checks/check_workflow.py --repo . --all-specs` |

## 并行拆分

- 第一阶段可并行：`SP144-T0` 仅拥有 graph migration contract；`SP144-T1` 仅拥有
  store schema/transaction。两条 lane 不共享可写文件。
- 第二阶段串行：`SP144-T2` 在 T0 稳定后冻结外部 report/API；`SP144-T3` 等待
  T0/T1/T2，避免 server apply 与 store transaction 同时漂移。
- 第三阶段：T2/T3 API contract 冻结后，`SP144-T4` 负责 Web 数据层；T5 只在 T4
  完成后接 UI。不得让两个 lane 同时写 `web/src/store.ts`、`store-types.ts`、
  `run-panels.tsx` 或同一测试文件。
- `SP144-T6` 不与功能实现并行修改 production files，只负责汇总 fresh verification；
  任何失败回到唯一 owner lane 修复。

## 验证

- 每个 tranche 先运行该行 `Verify`，再提交；不得使用前一 session 的结果。
- Rust 改动完成后运行 `cargo check --workspace`，提交前运行 `cargo test --workspace`。
- Web 改动完成后运行 `cd web && npx tsc --noEmit`，提交前运行 `cd web && npm test`。
- Spec packet 运行
  `python3 checks/check_workflow.py --repo . --spec-dir specs/GH144` 与
  `python3 checks/check_workflow.py --repo . --all-specs`。
- PR merge readiness 另走 `github_pr_evidence.py` 与 `pr_gate.py`；green CI 不等于
  human final review 或 merge authorization。

## Handoff Notes

- 维护者已在 #144 评论
  `https://github.com/majiayu000/helixflow/issues/144#issuecomment-5084344567`
  记录 product/tech spec approval 与 tasks/CI 推进授权。
- Spec PR 为 #148，原分支 `spec/gh144-v1-migration-entry`。implementation 必须在
  #148 通过 human final review 并合入后另开实现分支/PR；不得把 production code
  混入 spec PR。
- 本任务只迁移 workspace current version；historical v1 version 保持 immutable。
  #145 删除 legacy read path 前仍需定义历史 v1 的隔离或再迁移策略。
- `HELIXFLOW_V1_MIGRATION_APPLY` 只控制本期 apply 灰度，不得复用或替代
  `HELIXFLOW_AGENT_INTENT_CONTRACT`。
- API boundary 使用 camelCase，Rust/database/stable IDs 使用 snake_case；不得添加
  runtime alias、silent fallback 或未声明字段。
- `web/src/store.ts`、`api.ts`、`types.ts` 已接近 U-16 上限，新逻辑必须保持模块化；
  若实现导致任一文件超过 800 行，先拆分再继续。
- 当前授权不包含 final approval、merge 或 release；这些 human gates 必须在对应阶段
  单独记录。
