# Task Plan

## Linked Issue

GH-144

## Spec Packet

- Product: `product.md`
- Tech: `tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP144-T0` | graph lane | none | 固化 graph migration typed contract：为 unresolved/failed 分支增加稳定 `MigrationReasonCode`，显式校验 source graph 与 migrated `WorkflowGraphV2` 结构，生成稳定 node/候选顺序，并让 `migrate_v1_to_v2` 接收 workspace connector context/restricted binding set，不从 title/provider default 或瞬时 health 推断。文件所有权：`crates/graph/src/graph_v2.rs`、必要的新 migration module、`graph_v2_tests.rs`。 | 同一 graph/catalog/connector/migration version 的 report 逐字节稳定；source/result 结构错误使用顶层稳定 code；unknown node、model missing/ambiguous、capability mismatch、binding/default missing、cross-connector 均返回稳定 code；现有 topology/幂等测试不削弱。 | `cargo test -p helixflow-graph` |
| `SP144-T1` | store lane | none | 增加 assessment/migration audit 与原子 store contract：下一条 SQL migration 以 table rebuild 扩展 `versions.source` CHECK 并保留旧数据/index/FK/`semantics_json`，新增 append-only secret-free assessment、`version_migration_records.rs`、`VersionSource::Migration`、operation fingerprint/replay/conflict/current CAS。文件所有权：`crates/store/migrations/*version_migration*.sql`、`crates/store/src/version_migration_records.rs`、相关 store tests、`crates/store/src/lib.rs` 的最小 module/export/enum 接线。 | 真实旧 schema fixture 升级无损且可插入 `source=migration`；dry-run status/reason 与 apply conflict 可聚合且无 raw params；target/current/audit 同 transaction；source 不变；same operation replay、different payload conflict、并发与 fault injection 均满足 spec。 | `cargo test -p helixflow-store` |
| `SP144-T2` | server dry-run lane | `SP144-T0`, `SP144-T1` | 实现 migration DTO、canonical `reportHash` 与 dry-run route；读取 workspace connector identity，验证 workspace/current/version、graph path/hash/JSON/structure、migrated v2 structure 与现有 semantics，将 typed graph report 转为含顶层 failure、`workspaceConnectorId`、server `applyEnabled` 的 secret-free API，并追加 assessment。文件所有权：新 `crates/server/src/version_migration_routes.rs` 的 read/report 部分、独立 tests、`main.rs` route/module 最小接线。 | dry-run 只追加 secret-free assessment，不改变 graph/version/current/proposal/run；四种状态可测；source 级失败不合成 node；跨 connector、corrupt/structurally invalid graph 与 invalid result/semantics fail-closed；`applyEnabled` 不参与 `reportHash`；route 继承 auth。 | `cargo test -p helixflow-server version_migration` |
| `SP144-T3` | server apply lane | `SP144-T0`, `SP144-T1`, `SP144-T2` | 实现 route 入口的 `operationId` replay lookup、connector-aware apply precondition 重算、`CandidateKind::Migration` publish/store/mark-or-cleanup 协议和 `HELIXFLOW_V1_MIGRATION_APPLY` gate。文件所有权：`version_migration_routes.rs` 的 apply 部分、`version_file_consistency.rs` 的最小 candidate kind 扩展、server apply/concurrency/fault tests。 | committed operation 在 current/report/flag/candidate 前按 fingerprint replay；different payload 冲突；新 operation 的篡改/stale connector/report 返回 conflict；unresolved/disabled 不写 version；成功创建一个 v2 current 且原 v1 不变；失败全量暴露；same/different operation 并发满足 spec。 | `cargo test -p helixflow-server version_migration && cargo test -p helixflow-store version_migration` |
| `SP144-T3A` | version writer lane | `SP144-T0`, `SP144-T1`, `SP144-T3` | 审计并修复所有 production version writer 的 `semantics_json` 传播，包括 `proposal_routes.rs`、`ops_routes.rs`、`version_routes.rs`、`layout_routes.rs`、`workbench_message_graph.rs`、`workspace_events.rs`、`sweep_support.rs`、`workspace_canvas.rs` 及搜索发现的 writer。layout 复制 current；restore 复制 target；ops/proposal 保留未变节点、删除已删节点，新建/重类型 executable 缺显式 semantics 时 fail-closed。文件所有权：上述 writer 及各自回归 tests；与其他 lane 不并行写共享 server 文件。 | migrated current 经每条派生路径后 pinned semantics 不丢失；删除节点不留孤儿 semantics；新增/重类型 executable 未提供合法 semantics 时显式拒绝；初始 v1 的 `None` 路径有测试证明不是 v2 降级。 | `cargo test -p helixflow-server && cargo test -p helixflow-store` |
| `SP144-T4` | frontend data lane | `SP144-T2`, `SP144-T3` | 增加独立 Zod DTO、API client 与 Zustand migration slice，覆盖顶层 failure、`workspaceConnectorId`、server `applyEnabled` 与 replay contract，不把业务逻辑塞入已接近 U-16 上限的 `api.ts`/`store.ts`/`types.ts`。文件所有权：`web/src/version-migration-types.ts`、`api-version-migration.ts`、`store-version-migration.ts`、对应 tests，以及 `store-types.ts`/`store.ts` 的最小组合接线。 | 所有 response 先经 Zod；workspace generation/version/connector 变化 abort 并丢弃 stale response；未知 apply 结果保留 operation ID 并安全重试；hydrate 后以 server truth 为准。 | `cd web && npx tsc --noEmit && npm test -- version-migration` |
| `SP144-T5` | frontend UI lane | `SP144-T4` | 在 version history 邻近增加独立 migration panel，覆盖 report 顶层/node failure、explicit confirm、server `applyEnabled`、conflict/failure/success 与 accessibility。文件所有权：`web/src/components/version-migration-panel.tsx`、组件 CSS/测试、`run-panels.tsx` 的最小挂载。 | current version 才显示入口；顶层失败不伪装 node；每个 unresolved node 可定位；server disabled/loading/error/conflict/success 不互相伪装；键盘、label、`aria-live` 与 workspace 切换测试通过。 | `cd web && npx tsc --noEmit && npm test -- version-migration-panel` |
| `SP144-T6` | coordinator | `SP144-T0`–`SP144-T5`, `SP144-T3A` | 完成跨层验收、rollout evidence 与 handoff：运行全量 Rust/TS/SpecRail 检查，核对 P1–P17、secret-free assessment/apply contract、source immutability、derived semantics、candidate reconciliation，并记录 #145/#146 所需 migration coverage 字段。文件所有权：仅验证证据、PR body/handoff；发现问题回到所属 lane 修 production code，不弱化测试。 | 所有 deterministic checks fresh green；PR diff 与 GH144 specs/tasks 一致；无未解决 actionable review thread；明确 dry-run-first、apply flag 默认关闭及 rollback 证据；不宣称 merge authorization。 | `cargo check --workspace && cargo test --workspace && cd web && npx tsc --noEmit && npm test && cd .. && python3 checks/check_workflow.py --repo . --all-specs` |

## 并行拆分

- 第一阶段可并行：`SP144-T0` 仅拥有 graph migration contract；`SP144-T1` 仅拥有
  store schema/transaction。两条 lane 不共享可写文件。
- 第二阶段串行：`SP144-T2` 在 T0/T1 稳定后冻结外部 report/API；`SP144-T3` 等待
  T0/T1/T2，避免 server apply 与 store transaction 同时漂移。
- `SP144-T3A` 在 T3 后串行审计/修改 server version writers，不得与仍写相同 route
  文件的 lane 并发；必须 search-first 覆盖所有 `VersionRecordInput`/`semantics_json`
  writer，不能只修已知样本。
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
- 维护者已在当前会话明确授权合并 #148；仍必须把授权绑定最终 head，并在 merge 前
  重新运行 `github_pr_evidence.py` 与 `pr_gate.py`，任何 blocker 都不得绕过。授权不
  扩展到后续 implementation PR 的 merge 或 release。
