# Tech Spec

## Linked Issue

GH-144

## Product Spec

见 `specs/GH144/product.md`。

## Codebase Context

| Area | Files | Why relevant |
| --- | --- | --- |
| Migrator | `crates/graph/src/graph_v2.rs::migrate_v1_to_v2` | 已有确定性迁移 + `MigrationReport`（SP130-T2） |
| 版本文件 | `crates/server/src/version_file_consistency.rs` | `VersionFileCandidate::from_graph` + `publish_all` 原子发布，新增 `CandidateKind::MigratedGraph` |
| 版本创建 | `crates/store/src/version_records.rs::create_version_after` | expected-current 并发守卫；`NewVersion.semantics_json`（T6） |
| 语义层 | `versions.semantics_json`（migration 0006） | apply 持久化落点；非 NULL 即"已迁移" |
| 路由 | `crates/server/src/main.rs` | `POST /api/workspaces/{id}/graph-migration/{dry-run,apply}` |
| UI | `web/src/components/top-bar.tsx` + 新 `migration-panel.tsx` | 入口与面板 |

## 设计方案

- `crates/server/src/migration_routes.rs`：
  - `dry-run`：取 workspace 当前版本；`semantics_json` 非 NULL → alreadyMigrated；
    否则读图 → `migrate_v1_to_v2(graph, builtin registry, run shared_catalog)` →
    报告（camelCase，含逐节点 action 与计数）。纯读。
  - `apply`：同判定；resolvable → `VersionFileCandidate::from_graph(MigratedGraph)`
    发布 → `create_version_after`（label `v1→v2 migration`、source manual、
    semantics_json=Some、expected_current=当前版本）→ 返回新版本 id + 报告。
    needs_resolution → 409 `MIGRATION_NEEDS_RESOLUTION` + 报告。
- 幂等由"当前版本已带语义层"判定承担；并发由 store 的 expected-current 承担。
- UI：TopBar 在当前版本无语义层时显示"迁移"入口；`MigrationPanel` 拉 dry-run，
  展示计数 + 节点/原因列表，确认后 apply 并刷新 workspace state。

## Product-to-Test Mapping

| Invariant | Verification |
| --- | --- |
| 1 dry-run 确定性/零写入 | route 测试：两次调用报告相等；版本数不变 |
| 2 needs_resolution 拒绝 | 带无法解析模型的图 → 409 + reason，无新版本 |
| 3 幂等 | apply 两次 → 第二次 alreadyMigrated，版本数不变 |
| 4 原子/并发 | expected-current 冲突路径（store 已有测试覆盖守卫） |
| 5 topology 不变 | 迁移前后 nodes/edges 对比 |

## 风险

- Security：只读报告不含敏感字段；apply 走既有鉴权层。
- Compatibility：新版本追加，不改写历史；回滚 = restore。
- Maintenance：复用 migrator 与候选文件机制，无新持久化面（除 CandidateKind 枚举值）。

## 回滚方案

apply 产物是普通版本：restore 原版本即回滚；无 flag。
