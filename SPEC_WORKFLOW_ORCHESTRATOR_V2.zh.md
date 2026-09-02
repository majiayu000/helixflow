# Helixflow 工作流编排器 v2 产品合同

状态：规范性产品合同
版本：v2
日期：2026-09-03

本文定义 Helixflow 当前产品循环和必须保持的系统边界。历史 v1 合同保留在
`SPEC_WORKFLOW_ORCHESTRATOR.md`，只用于追溯，不再决定新增功能的行为。

## 1. 产品定位

Helixflow 是 local-first 的 AI 工作流编排器。用户用自然语言或画布编辑表达目标，
后端把高层意图确定性地编译为可执行图，生成不可变版本，并在成本闸之后调用
provider。它不是 ComfyUI 的薄客户端，也不是让 Agent 直接操作 provider 的外壳。

默认产品循环如下：

1. 用户描述工作流，或在画布上编辑。
2. 图编辑 Agent 只提交高层 `IntentPlan`。
3. 后端解析 capability、model 和 binding，编译并校验目标图。
4. 后端自动提交新的不可变执行版本；用户通过版本历史检查和回退。
5. 用户或 Agent 可以请求运行。后端负责估费、确认和实际 dispatch。
6. 用户检查逐节点进度、成本、错误和产物。

人工 proposal apply 是保留给已有手工 proposal API 的领域能力，不是 Agent 的输出
合同，也不是默认产品循环。

## 2. 强制边界

### 2.1 后端是真相源

- 前端不直接调用 Agent runtime、provider、本地文件或数据库。
- Agent 不持有 provider 凭据，不写数据库，不直接提交版本。
- WebSocket 用于增量通知，`GET /api/workspaces/{workspace_id}/state` 仍是重连后的
  对账入口。
- provider 未配置或 binding 不可用时必须 fail closed，不得静默换模型或能力。

### 2.2 Agent 负责语义，后端负责执行

图编辑 turn 的唯一 Agent 业务输出是 `intent.json`。Agent 可以提交
`run_request.json` 请求运行，但请求不等于执行授权。后端始终拥有：

- capability/model/binding 解析与校验；
- 图编译、版本提交和数据库写入；
- 成本估算、确认策略和额度检查；
- provider 调用、取消、恢复和 ledger；
- 失败的用户可见错误与状态。

动态工具与直接写入 `out/*.json` 是同一 `OutputContract` 的两种传输方式，必须进入
同一份后端校验，不形成两套业务协议。

### 2.3 不可变执行版本

- 每次语义图变化都创建新的不可变 `WorkflowVersion`。
- 每个 run 钉在明确的 version 和解析后的 binding 上。
- restore 可以直接复用既有 graph snapshot；initial、migration 和 restore 不需要伪装成
  graph ops。
- 所有新执行版本经过同一个 durable `commit_version` 边界。该边界协调 graph 文件
  publish/hash、SQLite 事务、current-version CAS 和失败清理。
- 启动时 reconciliation 是崩溃恢复保护，不替代正确的提交边界。

### 2.4 成本与可追溯性

- 所有付费 provider 调用必须经过同一成本政策。
- 需要确认的 run 在 dispatch 前进入明确的 waiting-confirmation 状态。
- 每次 provider 调用关联 run step 和 cost ledger。
- 自动 retry 不得绕过成本确认，也不得覆盖原 run/version 的审计记录。

## 3. 规范数据模型

### 3.1 Capability schema

静态 canonical capability schema 是能力共同字段的规范来源，至少定义：

- 稳定 `capability_id`；
- 稳定可执行 `node_type`；
- 媒体 category、展示名称和说明；
- typed inputs/outputs；
- 参数 schema。

系统保留三类不同投影，不物理合并：

- `NodeRegistry` 是结构节点和可执行图校验投影；
- `CatalogSnapshot` 是 capability/model/binding/connector 的静态解析合同；
- `ProviderCatalog` 是运行时 provider 可用性报告。

Capability node definition 从 canonical schema 派生。只有存在 enabled binding 的能力才
进入当前可执行节点投影；运行时 availability 仍在解析和运行前单独检查。结构节点
（例如 input/output）不冒充 capability。

### 3.2 Intent 与执行图

- `IntentPlan` 表达用户目标、拓扑、阶段、capability 和可选 model。
- compiler 决定 node id、typed wiring、binding selection、semantics 和初始布局。
- `WorkflowGraph` 是 provider-neutral 的可执行 snapshot，不是所有 UI 状态的容器。
- `ExecutionPlan` 在创建 run 时冻结，不受之后的 canvas 或 catalog 变化影响。

### 3.3 Canvas snapshot

空间状态与执行版本分开持久化：

- node position、size、viewport 等纯空间字段属于 workspace canvas snapshot；
- `WorkflowGraph.pos/size` 在当前 graph schema 中只作为新节点和首次投影的布局 seed；
  snapshot 建立后不再把空间更新写回执行版本；
- snapshot 使用单调 revision 和 compare-and-swap；stale revision 明确返回冲突；
- move、resize、layout 和 viewport 更新只推进 canvas revision，不创建执行版本；
- 节点参数、边、capability、binding semantics 等执行变化仍提交新 version；
- GET canvas 时由后端把当前执行图与 canvas snapshot 投影为前端文档；
- presence 保持临时状态；评论维持其明确的持久化边界。

v2 不要求 canvas op log、CRDT、事件重放或多用户离线合并。

## 4. 故障域

以下机制必须分开建模：

- execution retry/self-heal：同一执行计划的受预算重试；
- provider recovery：恢复已经 dispatch 的远端任务；
- version reconciliation：检查数据库引用与 graph 文件完整性；
- 用户 debug/rerun：显式的产品操作。

它们可以共享 correlation id、产品文案和成本政策，但不得共享一个笼统的持久化
状态机或 attempt 上限。

默认关闭且没有完整触发、成本、版本/run 关联和 UI 状态的自动 `agent-fix` 不属于 v2
产品合同，应从运行时产品面删除。显式 Debug Workflow turn 保留。

## 5. 当前拓扑和控制点

v2 的高层 Intent 只承诺 `linear` 和 `parallel`。执行器可以按 DAG ready queue 并行，
但这不自动授权新增 conditional、loop 或图中 HITL。

当前人工控制点是：

- 付费 run 的成本确认；
- 不可变版本历史与 rollback；
- 输出 accept/reject；
- 显式 debug 与 rerun。

## 6. 当前明确不做

- 通用 canvas op log、CRDT 或协作事件平台；
- 把 initial/restore/migration 强制转换成 `apply_ops`；
- 合并 retry、provider recovery 和文件 reconciliation；
- 单次万能 action envelope；
- conditional、loop 和图中 HITL；
- 描述型通用 HTTP connector DSL；
- ComfyUI workflow backend；
- provider marketplace、云托管和多用户账号系统。

这些项目需要独立用户故事、风险分析和验证计划，不能从“长期”或“通用”质量目标
推导为本合同的隐含工作。

## 7. 验收条件

v2 收口完成至少满足：

1. Agent 图编辑不存在切回 `proposal.json` 的环境变量或运行时分支。
2. capability 到 node type、端口和参数只在 canonical schema 中定义一次。
3. 所有创建新执行 snapshot 的主路径通过 durable version commit 协调器；纯 layout
   不再创建 `WorkflowVersion`。
4. Canvas 空间更新使用持久化 revision CAS，语义版本保持不变。
5. 默认关闭的自动 agent-fix 产品面已删除，self-heal、provider recovery、debug turn 和
   version reconciliation 仍分别工作。
6. 现有成本闸、immutable history、rollback、后端图校验和 provider fail-closed 行为均
   有回归测试。

## 8. 文档优先级

本文是当前规范性产品合同。冲突时按以下顺序判断：

1. 用户当前明确要求与仓库 `AGENTS.md`；
2. 本 v2 合同；
3. 专题 RFC 和实现文档；
4. `SPEC_WORKFLOW_ORCHESTRATOR.md` v1 与 `specs/` 历史记录；
5. non-normative 调研与架构分析。
