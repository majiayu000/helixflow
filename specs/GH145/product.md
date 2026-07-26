# Product Spec

## Linked Issue

GH-145

## 用户问题

运行期同时存在 `image_generate`（legacy capability id）与 `text_to_image`
（canonical），靠 `canonical_capability` 映射粘合；图的语义层以独立 map 旁挂
（`WorkflowGraphV2 = base + semantics`）。双重 id 与双层 schema 增加每一处校验、
迁移与删除 legacy 的心智与回归成本（U-24 别名、#130 P2/P8 的长期一致性）。

## 目标

- 全栈（registry/gateway/provider/API/UI/审计事件）统一 `text_to_image`。
- 删除 `canonical_capability` 运行期映射，无别名、无静默回退。
- `GraphNode` 内嵌 `semantics`（capability/mode/implementation），删除分层
  `WorkflowGraphV2`，单一 canonical graph schema。

## 非目标

- 不动 legacy proposal 契约与回滚开关（#146 的 release gate）。
- 不设计迁移 API/UI（#144 已交付）。

## Behavior Invariants

1. 改名与合并后，旧 durable 图（无内嵌语义）仍可读取并通过 #144 入口迁移；任何
   不可读取情形都是显式错误，不是静默回退。
2. 改名前创建的 run（plan_json 内 capability 为 legacy id）重试时显式失败并给出
   稳定错误码，绝不静默换能力执行。
3. pinned/policy 语义在 schema 合并前后逐字段等价（迁移器输出对比测试）。
4. `image_generate` 字符串在生产代码（specs/测试 fixture 注释除外）零残留。
5. 部署顺序保证：先发 schema 兼容读取版本，存量迁移达标后再发删除映射版本。

## 验收标准

见 issue #145 验收清单；补充：
- [ ] 双向 serde 兼容测试：旧图文件（无 semantics 字段）读取 → 迁移 → 新 schema
  写出 → 再读取幂等。
- [ ] 旧 run plan_json 重试路径的显式失败测试（稳定错误码）。

## 发布说明

两阶段发布（详见 tech.md rollout）；阶段间以 #144 迁移证据
（`versions.semantics_json` 非 NULL 比例）为 gate。
