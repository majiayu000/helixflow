# Tech Spec

## Linked Issue

GH-145

## Product Spec

见 `specs/GH145/product.md`。

## Codebase Context

| Area | 现状 | 变更 |
| --- | --- | --- |
| registry `builtin.rs` | `image.generate` 节点 capability=`image_generate`；estimated_cost catalog_key 同名 | capability/catalog_key → `text_to_image`（node_type `image.generate` 不变，durable 图不受影响） |
| gateway atlas/fal/mock/lib | capability 匹配串 `"image_generate"`（约 13 文件） | 全部改 `text_to_image`；对 legacy 串返回 `UnsupportedCapability`（显式） |
| graph_v2 `canonical_capability` | legacy→canonical 映射（validate/migrator/compiler/run 使用） | 阶段 2 删除；migrator 改为断言 def.capability 已 canonical |
| `WorkflowGraph`/`GraphNode` | `deny_unknown_fields`；66 处 struct literal | `GraphNode` 增 `#[serde(default, skip_serializing_if)] semantics: Option<NodeSemanticsEntry>`；`WorkflowGraph` 增可选 `catalog_revision`；literal 由编译器驱动机械补齐 |
| `WorkflowGraphV2` | base+semantics 分层，validate/migrate 入口 | 合并为 `WorkflowGraph` 上的方法；`versions.semantics_json` 保留为持久化编码（内容改为从节点收集/回填），或迁移为图文件内嵌（二选一，见备选） |
| 消费方 | compiler graph_builder、run resolved、server workbench_intent/migration、web types | 读写点全部切到内嵌字段 |

## 设计方案（两阶段 rollout）

### 阶段 A（一个 PR）：schema 合并 + 兼容读取
1. `GraphNode.semantics: Option<NodeSemanticsEntry>`（serde default）。旧图文件缺
   字段 → None，读取不破。
2. `WorkflowGraphV2` 删除；`validate_semantics(catalog)` / `migrate_v1`(产出内嵌
   语义的新图) 移到 `WorkflowGraph`。`versions.semantics_json` 写入路径改为
   序列化"节点内嵌语义的汇总"（读侧兼容两种来源，优先节点内嵌）。
3. compiler 产出内嵌语义节点；T6 的 auto-apply 与 #144 apply 同步切换。
4. run `load_version_semantics`：优先图内嵌，回退 semantics_json 列（老迁移版本）。
5. capability 改名（registry/gateway/全栈）+ 旧 run 重试显式失败测试。
   `canonical_capability` 此阶段保留（migrator 仍需识别旧 registry 数据？不——
   registry 是代码内数据，改名后 migrator 读到的即 canonical；映射变为恒等，标记
   deprecated 待删）。
6. cache key schemaVersion 3→4（capability 串变了）。

### 阶段 B（第二个 PR，gate：存量迁移证据）
1. 删除 `canonical_capability` 与 semantics_json 列回退读取（migration 0007 可选
   清列）。
2. `image_generate` 零残留断言测试（rg gate 脚本）。

## 备选方案

- 语义持久化改为图文件内嵌（删 semantics_json 列）：更单一，但要重写 #144 迁移
  证据查询（按列计数）；保留列作为索引缓存更实用 — 采用"列=派生索引"。
- 一次性大 PR：拒绝——rollout 顺序无法保证旧数据可读窗口。

## 风险

- Compatibility：最高风险在旧 run 重试与 in-flight sweep；阶段 A 显式失败 + 文档。
- 回归面：66 literal + 全部消费方；靠编译器驱动 + 现有 500+ 测试 + serde 双向
  幂等测试兜底。

## 测试计划

- serde：旧图 JSON（fixture 复制真实文件）读取/迁移/写出/再读幂等。
- 等价：同一 v1 图经"旧分层迁移器"与"新内嵌迁移器"产出的语义逐字段相等
  （阶段 A 内保留旧实现做对照测试后删除）。
- 改名：gateway 三 provider 对 legacy 串显式拒绝；catalog/preflight/审计事件用
  canonical。
- 全量：`cargo test --workspace` + web 全量 + `check_workflow.py`。

## 回滚方案

阶段 A 整 PR revert 安全（新字段 optional，旧代码忽略 → 需验证 deny_unknown：旧
代码 `deny_unknown_fields` 会拒新图!!）→ 因此阶段 A 必须先于任何新图写入部署；
revert 窗口 = 首个内嵌语义版本写入前。此约束写入发布说明并在 PR 中显著标注。
