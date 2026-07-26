# Tech Spec

## Linked Issue

GH-130

## Product Spec

见 `specs/GH130/product.md`。

本文迁移自原本地草案 `specs/capability-driven-workflow-compiler.md`（已删除），
并按维护者决定移除了 ComfyUI backend 的本期范围。

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Registry | `crates/registry/src/lib.rs` | `NodeDefinition` 声明单一 capability、typed ports 和 params schema | 缺少 model、connector、binding 和 model-mode schema；catalog V2 的落点 |
| Gateway 抽象 | `crates/gateway/src/lib.rs`、`registry.rs`、`runtime_provider.rs` | `Provider` trait（id/fingerprint/health/invoke）+ `ProviderRegistry` 多 provider 聚合；catalog 只描述 capability/output | 没有 model、输入模式、binding、revision 和 availability |
| Atlas/Fal adapter | `crates/gateway/src/atlas.rs:195-307`、`crates/gateway/src/fal.rs:14` | 读取 `params.model`，缺失时静默回退内部默认（`google/nano-banana-2/text-to-image`、`bytedance/seedance-v1.5-pro/text-to-video-fast`、`fal-ai/nano-banana-2`） | 静默不一致的直接根因；改为只执行已解析的 `operation_id + model_id` |
| Graph | `crates/graph/src/lib.rs` | `WorkflowGraph`/`GraphService`/`ExecutionPlan`；`GraphNode` 保存 node type、title、params、position | `params.model` 是未声明的执行事实；Graph V2 增加 `semantics` 与 `implementation` |
| Agent contract | `crates/agent/src/runtime.rs`、`contract.rs` | `CodexRuntime` spawn `codex exec`，`ctx/instructions.md` 入、`out/proposal.json` 出 | Agent 直出低层 `ProposalOp`；改为输出 `out/intent.json` |
| Preflight | `crates/server/src/capability_preflight.rs` | HF-009：只验证 provider 是否支持 capability | 扩展为 model/binding/connector/resolved implementation 全量校验 |
| Run | `crates/run/src/lib.rs`、`cost_gate.rs` | `RunService` 执行、cost gate、sweep、self-heal | 每个 step 需固化 `ResolvedExecutionStep` 不可变快照 |
| Store | `crates/store/src/*` | run/version/proposal/retry/sweep/cost 记录 | 新增 catalog、binding、implementation snapshot 持久化 |
| 前端 | `web/src/components/graph-canvas*.tsx`、`node-library*`、`chat-pane.tsx` | 画布编辑、node library、Agent 面板 | capability/model 双视图、implementation inspector、澄清与拓扑预览 |

## 设计方案

### 1. 决策摘要

HelixFlow 增加独立的、能力驱动的 workflow 编译层。Agent 不再直接生成底层
`ProposalOp`，而是先输出结构化 `IntentPlan`；后端再通过 `CapabilityResolver`
和 `WorkflowCompiler` 确定性地产生 typed `WorkflowGraph`、proposal diff、
布局和执行计划。

能力、模型、连接器是多对多关系，不以树形目录作为事实源：

```text
CapabilityDefinition
          ↑
          │
  CapabilityBinding ───→ ModelDefinition
          │
          ├────────────→ ConnectorDefinition
          └────────────→ WorkflowBackendDefinition / WorkflowTemplate
```

树形结构只用于 UI 投影。一个模型可以实现多个 capability，一个 capability 也可以
由多个模型实现。

根因不是 prompt 写得不够好，而是系统缺少可执行的模型绑定契约。Prompt 不能替代
catalog lookup、schema validation、typed port unification、exact-model
resolution 和 run-time preflight。

### 2. 强制不变量

| ID | 不变量 |
| --- | --- |
| P1 | `title`、坐标和布局不参与 capability、model 或 topology 判定。 |
| P2 | capability、model、connector、backend 必须来自指定 catalog revision。 |
| P3 | capability 与 model 是多对多关系，只有有效 `CapabilityBinding` 可执行。 |
| P4 | pinned selection 的 requested/resolved model 必须一致，否则失败。 |
| P5 | policy selection 只能使用明确配置的默认 binding；多个无默认候选时澄清。 |
| P6 | `linear` 不得编译出语义 fan-out；`parallel` 必须来自用户明确意图。 |
| P7 | 每个必需输入必须由用户输入、合法参数或兼容 typed edge 满足。 |
| P8 | capability、model、port 或 connector 问题不得静默降级。 |
| P9 | Agent action 不直接修改 durable graph；仅验证后的 proposal 可原子应用。 |
| P10 | backend 展开细节不进入 durable semantic graph。 |
| P11 | 每个 run 固化 catalog、binding 和 resolved implementation snapshot。 |
| P12 | run 创建后的 catalog 更新不得改变该 run。 |
| P13 | endpoint、credential、auth header 不进入 catalog、graph、Agent ctx 或前端。 |
| P14 | migration 不根据 title 猜模型，不改变已有 edge topology。 |
| P15 | API 边界使用 camelCase；Rust 内部字段与稳定 ID 使用 snake_case。 |

### 3. 领域模型

#### 3.1 CapabilityDefinition

```rust
pub struct CapabilityDefinition {
    pub capability_id: String,
    pub category: MediaCategory,
    pub display_name: String,
    pub inputs: Vec<PortDefinition>,
    pub outputs: Vec<PortDefinition>,
    pub params_schema: ParamsSchema,
}
```

V2 初始 canonical capability IDs：

```text
prompt_writer
text_to_image
image_edit
text_to_video
image_to_video
video_extend
upscale_image
upscale_video
image_analyze
```

现有 `image_generate` 通过一次性 migration 转为 `text_to_image`，不保留运行期
alias。`text_to_video` 保持不变。

#### 3.2 Model、Connector 与 Backend

```rust
pub struct ModelDefinition {
    pub model_id: String,
    pub family_id: String,
    pub display_name: String,
    pub vendor: String,
    pub lifecycle: ModelLifecycle,
}

pub struct ConnectorDefinition {
    pub connector_id: String,
    pub provider_id: String,
    pub kind: ConnectorKind,
    pub enabled: bool,
    pub status: ConnectorStatus,
}

pub struct WorkflowBackendDefinition {
    pub backend_id: String,
    pub kind: WorkflowBackendKind,
    pub enabled: bool,
    pub status: BackendStatus,
    pub catalog_revision: String,
}
```

`model_id` 是 canonical execution identity，不是 UI nickname。用户输入的
"Nano Banana""Seedance 2"必须先解析；Graph 不保存未解析字符串。

Connector 对外只暴露安全元数据。endpoint、credentials 和 auth 配置仅存在于
gateway implementation。

`WorkflowBackendDefinition` 本期仅作为抽象位保留（P10），无任何 backend 实现。

#### 3.3 CapabilityBinding

```rust
pub struct CapabilityBinding {
    pub binding_id: String,
    pub capability_id: String,
    pub model_id: String,
    pub implementation: ImplementationTarget,
    pub mode: String,
    pub input_schema: ParamsSchema,
    pub output_schema: ParamsSchema,
    pub defaults: Value,
    pub availability: Availability,
    pub binding_revision: String,
}

pub enum ImplementationTarget {
    ApiConnector {
        connector_id: String,
        operation_id: String,
    },
    WorkflowTemplate {
        backend_id: String,
        template_id: String,
        template_revision: String,
    },
}
```

示例（数据，不是代码分支）：

```yaml
- binding_id: google.nano-banana-2.text-to-image.v1
  capability_id: text_to_image
  model_id: google/nano-banana-2
  mode: text_to_image
  implementation:
    api_connector:
      connector_id: atlas
      operation_id: generate_image

- binding_id: bytedance.seedance-v1-5-pro.text-to-video.v1
  capability_id: text_to_video
  model_id: bytedance/seedance-v1.5-pro
  mode: text_to_video
  implementation:
    api_connector:
      connector_id: atlas
      operation_id: create_video
```

V1 种子 catalog 以现有真实调用为准（atlas：nano-banana-2、seedance-v1.5-pro；
fal：nano-banana-2），最终清单由维护者在 spec 审批时确认。

#### 3.4 ImplementationSelection

```rust
pub enum ImplementationSelection {
    Pinned {
        requested_model_id: String,
        binding_id: String,
    },
    Policy {
        policy_id: String,
        constraints: SelectionConstraints,
    },
}
```

用户点名模型必须生成 `Pinned`。只有用户未点名模型且产品策略明确允许自动选择时，
才能使用 `Policy`。V1 不允许 provider 在执行阶段再次自行挑选 model。

### 4. Agent Contract：IntentPlan

```rust
pub struct IntentPlan {
    pub intent_version: String,
    pub topology: TopologyIntent,
    pub stages: Vec<StageIntent>,
    pub output_stage_ids: Vec<String>,
    pub assumptions: Vec<String>,
}

pub enum TopologyIntent {
    Linear,
    Parallel,
}

pub struct StageIntent {
    pub stage_id: String,
    pub capability_id: String,
    pub requested_model: Option<String>,
    pub input_from: Vec<StageInputRef>,
    pub params: Value,
}
```

Graph 创建、扩展和替换模式输出 `out/intent.json`，不再让 Agent 直接输出
`proposal.json`。允许的高层 action：

- `create_media_pipeline`
- `insert_stage`
- `remove_stage`
- `replace_stage_implementation`
- `set_stage_params`
- `set_topology`

Agent 不得设置 durable node ID、edge ID、position、binding ID、connector ID 或
backend payload。`stage_id` 只在单次 intent 中用于引用。

以下情况必须按 `routing-contract.md` 返回 `clarify_first`：

- 下游是 `image_to_video`，但没有 image 输入、上游 image output 或已选素材；
- 用户要求串行，但相邻 stage 的 typed ports 不兼容；
- 模型名称解析出多个 canonical model；
- 指定模型不存在，或没有目标 capability 的 binding；
- 多个候选 binding 且没有明确默认策略；
- 用户表达同时包含互相冲突的串行与并行要求。

共享 handoff 至少包含 `route`、`reason_code`、`missing_fields`、
`safe_context` 和可恢复的 `next_action`。

### 5. CapabilityResolver

输入：`capability_id`、可选 `requested_model`、可选 `backend_preference`、
workspace connector/backend 状态、`catalog_revision`。

成功输出：

```rust
pub struct ResolvedImplementation {
    pub capability_id: String,
    pub requested_model_id: Option<String>,
    pub resolved_model_id: String,
    pub binding_id: String,
    pub binding_revision: String,
    pub target: ImplementationTarget,
}
```

解析顺序固定：

1. 校验 capability 存在。
2. 归一化用户模型名；0 个结果为 unknown，多个结果为 ambiguous。
3. 查找 `(capability_id, model_id)` 的 active bindings。
4. 按 workspace availability、backend preference 和显式 policy 过滤。
5. pinned 结果必须唯一；policy 结果必须有唯一默认项。
6. 返回完整 resolved implementation，或 typed error/clarification。

禁止按 display title、字符串包含关系、provider 内部默认值或"最接近模型"解析。

### 6. WorkflowCompiler

新增 `crates/compiler`：

```text
src/
  lib.rs
  intent.rs
  resolver.rs
  topology.rs
  graph_builder.rs
  proposal_diff.rs
  layout.rs
  errors.rs
```

编译输入：已通过 schema validation 的 `IntentPlan`、当前 `WorkflowGraph`、
catalog snapshot、workspace availability snapshot、compile mode
（create / extend / replace）。

编译输出：

```rust
pub struct CompiledProposal {
    pub graph_schema_version: String,
    pub catalog_revision: String,
    pub ops: Vec<ProposalOp>,
    pub resolved_stages: Vec<ResolvedStage>,
    pub layout_hints: Vec<LayoutHint>,
    pub diagnostics: Vec<CompileDiagnostic>,
}
```

确定性算法：

1. 校验 intent schema、stage 引用和 topology。
2. 为每个 stage 解析 capability/model/binding。
3. 从 capability definitions 取得 typed inputs/outputs。
4. 根据 `input_from` 和 topology 做端口统一。
5. 缺少必需输入时停止并返回 `clarify_first`。
6. 生成稳定 node/edge ID；相同输入产生相同 proposal。
7. 创建 semantic graph，并写入 implementation selection。
8. 运行 graph validation、binding validation、topology validation。
9. 生成最小 proposal diff 和 layout hints。
10. 所有检查通过后才允许 proposal preview/apply。

`Linear` 必须形成一条语义主链。`Parallel` 只能按 intent 中明确的分支生成
fan-out；不能因为存在多个模型候选而自动并行。

### 7. WorkflowGraph Schema V2

```rust
pub struct WorkflowGraph {
    pub schema_version: String,
    pub catalog_revision: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

pub struct GraphNode {
    pub node_id: String,
    pub node_type: String,
    pub title: String,
    pub params: Value,
    pub position: Position,
    pub semantics: NodeSemantics,
    pub implementation: ImplementationSelection,
}

pub struct NodeSemantics {
    pub capability_id: String,
    pub mode: String,
}
```

`params` 只包含 capability/binding schema 声明的生成参数，不再允许
`params.model` 作为隐式执行字段。Graph validator 新增：

- catalog revision 可读取；
- capability、model、binding 均存在；
- binding 与 capability/model 匹配；
- connector/backend 可用于计划生成；
- node params 同时满足 capability 和 binding schema；
- edge artifact type 与 required port 匹配；
- pinned requested/resolved identity 可验证。

### 8. 执行：ResolvedExecutionStep

现有 executor 继续执行语义节点，但每个 step 必须保存不可变实现：

```rust
pub struct ResolvedExecutionStep {
    pub node_id: String,
    pub capability_id: String,
    pub requested_model_id: Option<String>,
    pub resolved_model_id: String,
    pub binding_id: String,
    pub binding_revision: String,
    pub connector_id: Option<String>,
    pub backend_id: Option<String>,
    pub params: Value,
    pub backend_payload_hash: String,
}
```

Gateway 接收 `ResolvedExecutionStep`，不能再根据 capability 或缺失参数选择默认
model。Atlas/Fal adapter 只执行已经解析的 `operation_id + model_id`；删除
`DEFAULT_FAL_IMAGE_MODEL` 与 atlas 的 `unwrap_or_else` 默认模型分支。

### 9. Catalog 与 API

统一 catalog：

```json
{
  "catalogRevision": "sha256:...",
  "capabilities": [],
  "models": [],
  "bindings": [],
  "connectors": [],
  "workflowBackends": []
}
```

新增或升级接口：

- `GET /api/catalog`
- `GET /api/catalog/capabilities/:capabilityId/models`
- `GET /api/catalog/models/:modelId/capabilities`
- `POST /api/catalog/resolve`
- `POST /api/workflows/compile-intent`
- `POST /api/workflows/:id/preflight`

Agent context 只接收完成当前任务所需的安全切片：canonical ID、display name、
capability、模式、typed ports、可用状态和必要参数摘要。不得包含密钥或 endpoint。

### 10. Run Preflight 与审计

创建 run 前按顺序验证：

1. graph schema 与 graph version；
2. catalog revision 或显式 re-resolve 流程；
3. 每个 binding/model/capability 的一致性；
4. connector/backend availability；
5. typed input completeness；
6. capability/binding params schema；
7. pinned exact-model；
8. cost gate；
9. 生成并持久化 immutable implementation snapshot。

任一失败都阻止 run，不允许 warning + fallback。Run event 和 trace 至少记录：
graph/version ID、catalog revision、requested/resolved model、binding/revision、
connector、backend payload hash、artifact provenance、error code 和失败阶段。

### 11. Graph V1 → V2 迁移

提供显式 migrator 与 dry-run report：

1. `image_generate` 转换为 `text_to_image`。
2. 读取已声明且已验证的 legacy model 字段。
3. 缺少模型时不采用 provider 默认值，标记 `needs_resolution`。
4. model 无法唯一映射时标记 `needs_user_choice`。
5. 不从 title 推断模型。
6. 保留 node/edge ID、位置、title、参数和 topology。
7. 写入 `migration_version`、source graph version 和 diagnostics。
8. 只有全部节点可解析时才允许提交 v2 version。

迁移必须可 dry-run、可回滚。v1 读取路径保留到 T6 完成，v2 写入后不得降级覆盖 v1。

### 12. UI 与 Canvas

Node library 提供两种事实一致的视图：按 capability 浏览展开可用模型；按 model
浏览展开其 capabilities。树只是 catalog 的投影，不产生独立配置。

Inspector 明确展示：capability 与 mode、pinned/policy、requested/resolved
model、connector、binding revision 与不可运行原因。

Agent draft 在应用前展示 topology、阶段、模型选择、缺失输入、编译后的 graph
diff。澄清状态不得伪装成成功 proposal。

布局由 compiler/layout service 根据语义拓扑生成：串行主链横向排列；明确的并行
分支才纵向展开。布局不影响执行语义，用户移动节点不会改变 topology。

### 13. 错误模型

至少新增：

```text
CAPABILITY_NOT_FOUND
MODEL_NOT_FOUND
MODEL_AMBIGUOUS
BINDING_NOT_FOUND
BINDING_AMBIGUOUS
BINDING_UNAVAILABLE
MODEL_CAPABILITY_MISMATCH
PINNED_MODEL_MISMATCH
REQUIRED_INPUT_MISSING
PORT_TYPE_MISMATCH
TOPOLOGY_CONFLICT
CATALOG_REVISION_STALE
CONNECTOR_UNAVAILABLE
MIGRATION_NEEDS_RESOLUTION
```

每个错误包含稳定 `code`、安全的人类说明、相关 stage/node/field、是否可恢复、
`next_action`。服务端记录完整诊断，前端不得展示密钥、内部 URL 或 provider 响应。

### 14. 安全与运维

- catalog 注册和 connector 配置必须鉴权并审计。
- schema 与 payload 必须限制大小、深度和允许字段。
- 禁止任意 class、路径、shell、URL 和动态代码执行。
- provider 响应必须校验 artifact 类型与来源。
- catalog 和 binding 发布采用 revision + 原子切换。
- 指标至少包含 resolve/compile/preflight 成功率、澄清原因、binding 使用量、
  exact-model mismatch 和 catalog stale。

### 15. 分阶段实施

- **T0 fixtures 与 evaluator**：固化 Nano Banana/Seedance、GPT Image/Seedance、
  串行、并行、缺图、model mismatch fixtures；为现有行为补 regression tests。
- **T1 Catalog V2 与多对多 binding**：`crates/registry`、`crates/gateway`、
  catalog API 与 persistence。实现 capability/model/connector/binding、revision、
  availability 和 resolver；删除 provider adapter 的模型选择职责（保留 legacy
  兼容入口至 T6）。
- **T2 Graph V2 与 migration**：`crates/graph`、graph storage/API、migration
  command 与报告。
- **T3 IntentPlan、compiler 与 proposal diff**：`crates/agent`、新增
  `crates/compiler`、proposal service。
- **T4 ResolvedExecutionPlan 与 run preflight**：run/executor、
  `crates/gateway`、Atlas/Fal adapters。
- **T5 Workbench UX**：`web/src/components/**`、catalog/workflow API clients。
- **T6 切换默认路径与清理 legacy contract**：灰度启用 IntentPlan/compiler，
  停止低层 Agent proposal 输出，移除 `params.model`、provider defaults 和运行期
  `image_generate` 兼容。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| 1（点名即执行） | resolver + run preflight + adapter | preflight/adapter contract test：`PINNED_MODEL_MISMATCH` |
| 2（无默认必澄清） | resolver policy 分支 | resolver unit test：ambiguous/no-default |
| 3（串行不 fan-out） | compiler topology | topology invariant/property test |
| 4（缺输入必澄清） | compiler 步骤 5 | compiler `REQUIRED_INPUT_MISSING` 测试 |
| 5（title/位置无语义） | graph validator | graph validator + UI integration test |
| 6（数据上新模型） | catalog fixtures | Nano Banana ↔ GPT Image replacement test |
| 7（run 快照审计） | store snapshot records | run trace integration test |
| 8（无静默降级） | 错误模型全链路 | typed error assertion（全部错误码有测试） |
| 9（迁移保拓扑） | migrator | dry-run/幂等/拓扑不变测试 |
| 10（不泄密） | API/ctx 序列化层 | secret-free serialization test |
| 11（澄清即澄清） | 前端 proposal 面板 | web integration test |

## 数据流

- 输入：用户聊天消息 → Agent（Codex CLI，`ctx/` 安全切片）→ `out/intent.json`。
- 编译：server 校验 intent schema → `CapabilityResolver`（catalog snapshot）→
  `WorkflowCompiler` → `CompiledProposal` → proposal preview → 用户 apply →
  版本事务写入 Graph V2。
- 执行：run preflight（10 步）→ `ResolvedExecutionStep` snapshot 持久化 →
  `RunService` → `ProviderRegistry` → Atlas/Fal（只执行 resolved
  `operation_id + model_id`）→ artifact 校验与落盘 → run trace 记录审计字段。
- 外部调用：仅 gateway 层持有 endpoint/credential（P13）。

## 备选方案

- **继续加固 prompt 而不加编译层**：被否。根因是缺少可执行契约，prompt 不能替代
  catalog lookup、schema validation 与 preflight（见"决策摘要"）。
- **ComfyUI workflow backend 本期一并实现**：维护者已否决，过重。另立 issue；
  本期仅保留 `ImplementationTarget::WorkflowTemplate` 与
  `WorkflowBackendDefinition` 抽象位，避免后续接入时改动领域模型。
- **由 provider 在执行期解析模型**：被否。违反 P4/P11/P13，审计链断裂，且正是
  当前静默不一致的来源。

## 风险

- Security：catalog 注册与 connector 配置是新的写入面，必须鉴权、审计、限制
  schema 大小与字段（见"安全与运维"）。Agent ctx 切片须证明不含密钥/endpoint。
- Compatibility：Graph V1 → V2 迁移不可逆写入；靠 dry-run、`needs_resolution`
  标记与"v2 不降级覆盖 v1"约束控制。`image_generate` → `text_to_image` 是一次
  性 breaking migration，无运行期 alias（符合仓库无兼容层惯例，但要求迁移工具
  一次做对）。
- Performance：resolve/compile 为纯内存确定性计算，风险低；catalog snapshot
  读取需缓存，避免每次 compile 查库。
- Maintenance：新增 `crates/compiler` 与 catalog 数据面扩大了面积；靠 T0
  fixtures、golden proposal 测试和错误码全覆盖控制回归成本。

## 测试计划

- [ ] Unit tests：registry（唯一 ID、引用完整性、revision、many-to-many）；
  resolver（unknown/ambiguous/unavailable/pinned/policy）；compiler（linear、
  parallel、missing input、port mismatch、稳定 ID、幂等 diff）；graph（schema
  v2、binding 一致性、params、cycle、required ports）；migration（dry-run、
  不可推断模型、拓扑不变、幂等）；gateway（resolved model 透传、无默认
  fallback、artifact type）。关键路径 100% 覆盖，新增代码 ≥80% line coverage。
- [ ] Integration tests：
  1. `Prompt → Nano Banana(text_to_image) → Seedance(image_to_video) → Output`
     端到端 golden graph/run；
  2. Nano Banana 替换为 GPT Image，只变 binding/model，edge topology 不变；
  3. 缺 image 输入返回澄清，不创建错误 proposal；
  4. 明确 parallel 生成两分支，未明确不生成；
  5. catalog revision 在 compile/run 间变化时 run 被阻止；
  6. connector 下线后 preflight 失败，不自动换模型；
  7. v1 graph dry-run 不根据 title 或 provider default 填模型。
- [ ] Manual verification：Workbench 中完成验收场景 20.1-20.3（见 product.md
  验收标准），确认 inspector 展示 requested/resolved model 一致。

各阶段必跑命令（只执行与当阶段实际改动对应的命令，正式交付前覆盖所有 touched
code path）：

```sh
python3 checks/check_workflow.py --repo .
cargo check --workspace
cargo test --workspace
cd web && npx tsc --noEmit
cd web && npm test
```

## 回滚方案

Feature flags 按依赖顺序：

```text
catalog_v2
graph_schema_v2
intent_plan_agent_contract
workflow_compiler
resolved_execution_plan
workbench_implementation_browser
```

发布顺序严格按 T1→T6：先双读/影子编译和对比诊断，再灰度 v2 写入，最后关闭
legacy 写入。回滚只能切回兼容读取/旧执行路径，不能覆盖已经持久化的 v2 graph。
T6 之前任何阶段都可以单独关闭对应 flag 回到上一阶段行为。

实现前仍需维护者决定（spec 审批时确认）：

1. V1 默认支持的 canonical model/binding 清单。
2. policy selection 初期是否仅允许 workspace admin 配置唯一默认 binding。
3. v1 graph 的迁移截止时间与 legacy read path 删除版本。

Agent 不得自行 approve、merge、发布、改变权限或关闭争议事项。
