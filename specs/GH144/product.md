# Product Spec

## Linked Issue

GH-144

## 用户问题

存量 v1 工作区图没有语义层：模型选择只能靠配置默认解析，无法 pinned，也阻塞
`canonical_capability` 与 legacy 路径的删除（#145/#146）。维护者需要先看到迁移影
响（哪些节点可迁移、哪些需要人工解决），再用受控、可回退的方式完成迁移。

## 目标

- 只读 dry-run：按 workspace 当前版本返回确定性迁移报告，零持久化写入。
- 显式 apply：迁移成功时创建一个携带语义层的新版本（原图保留在历史，天然可回滚）。
- UI 呈现计数（可迁移 / needs_resolution / 已迁移）并能定位问题节点。

## 非目标

- 不删除 `canonical_capability` / legacy 契约 / 回滚开关（#145、#146）。
- 不做批量跨 workspace 迁移。

## Behavior Invariants

1. dry-run 对相同 graph + catalog revision 返回逐字节相同的报告，且不写任何数据。
2. apply 只在报告 resolvable 时执行；否则 409 + 每节点稳定 reason，零写入。
3. apply 幂等：当前版本已有语义层时返回 alreadyMigrated，不产生新版本。
4. apply 原子：新版本一次性创建（graph 文件 + semantics），并发修改被
   expected-current 守卫拒绝；不存在半迁移状态。
5. 迁移不改变节点/边 topology，不从 title 推断模型（继承 SP130-T2 migrator 不变量）。
6. 原版本完整保留在历史中，回滚 = 版本回退（现有 restore 能力）。
7. 响应不含 credential、endpoint、provider 原始响应。

## 验收标准

- [ ] dry-run 确定性 + 零写入（测试证明）。
- [ ] 报告含 status（ready/needsResolution/alreadyMigrated）与逐节点 action。
- [ ] apply 幂等 + 并发守卫 + 409 needs_resolution 路径测试。
- [ ] 迁移后 topology 不变（测试）。
- [ ] UI 展示计数与问题节点列表，apply 后刷新。

## 发布说明

审计标记：迁移版本 label 为 `v1→v2 migration`、source=manual、parent=原版本；
semantics_json 即迁移证据。回滚 = restore 原版本。
