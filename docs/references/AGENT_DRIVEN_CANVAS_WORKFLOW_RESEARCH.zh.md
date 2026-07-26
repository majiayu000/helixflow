# Agent 主导的 Canvas / Workflow 设计调研

状态：研究参考，不是已批准的产品或技术 Spec。

调研日期：2026-07-26

## 目的

本文记录 Agent 主导的画布与工作流产品调研，回答以下问题：

1. Agent 应如何理解画布状态。
2. Agent 应直接生成完整 graph，还是调用受约束的画布操作。
3. 自然语言中的模型、拓扑和数据流意图应如何落到可执行 workflow。
4. HelixFlow 应借鉴哪些交互与架构模式。
5. 哪些外部实现只适合参考 UX，不适合作为 HelixFlow 的执行语义。

本文重点调研：

- [basketikun/infinite-canvas](https://github.com/basketikun/infinite-canvas)
- [tldraw Agent starter kit](https://tldraw.dev/starter-kits/agent)
- [tldraw Workflow starter kit](https://tldraw.dev/starter-kits/workflow)
- [Vercel Workflow Builder Template](https://github.com/vercel-labs/workflow-builder-template)
- [OpenSail Architecture Panel](https://github.com/TesslateAI/OpenSail)
- [n8n AI Workflow Builder](https://blog.n8n.io/ai-workflow-builder-best-practices/)
- [Dify Workflow Studio](https://dify.ai/workflows)

源码分析固定到以下版本，避免后续上游变化导致结论无法复现：

- `infinite-canvas`: `a0287a5e346e219bd488d084295542213912d816`
- `tldraw`: `e857c8e412bef005cec63bef6c30807fab255ff7`
- `workflow-builder-template`: `24fb0fd4524ee10491ea6b73f6df94137ecbf9e1`

## 结论摘要

HelixFlow 最值得借鉴的不是某个画布组件，而是 Agent 与画布之间的中间层：

```text
用户意图
  ↓
意图解析与缺失信息检查
  ↓
能力解析：model / provider / capability / typed ports
  ↓
高层 Agent Action
  ↓
确定性编译为底层 graph ops
  ↓
类型检查、lint、layout
  ↓
Proposal 预览
  ↓
人工应用或满足策略后的自动应用
```

核心判断：

- Agent 不应主要依靠一次性生成完整 graph JSON。
- Agent 应优先调用高层、类型化、可验证的 canvas/workflow actions。
- 模型必须先通过能力目录解析，不能只写进节点标题。
- `linear`、`parallel`、`conditional` 等拓扑必须成为显式决策。
- 能力或端口缺失时应进入澄清或明确失败，不能悄悄改变拓扑。
- 语义 graph 与 visual layout 应分离；Agent 描述结构意图，确定性布局器负责坐标。
- HelixFlow 已有 proposal、版本和后端验证，应保留并加强，不应退化为浏览器直接修改的弱一致模型。

## 调研方法与证据边界

外部事实来自仓库 README、官方文档，以及固定 commit 下的 Agent schema、画布桥接、生成 API 与状态处理源码。带有“推断”“建议”“推荐”的内容是基于这些事实和当前 HelixFlow 实现形成的设计判断，不表示上游采用了相同架构，也不表示 HelixFlow 已批准相关改动。

本次不评测外部项目真实生成任务的成本、稳定性和性能，不做画布 SDK 选型、安全审计，也不定义 HelixFlow 的正式数据库迁移、API schema 或实现任务。

## 项目对比

| 项目 | Agent 操作模型 | Workflow 语义 | 人工控制 | HelixFlow 可借鉴点 | 主要限制 |
| --- | --- | --- | --- | --- | --- |
| `infinite-canvas` | MCP 高层工具最终归一为 canvas ops | 通用创作节点与弱类型连线 | 写工具确认、撤销 | 高层工具、状态先读、模型配置查询、事件流 | 不是强类型执行 DAG |
| tldraw Agent | 类型化 Action + Prompt Parts + sanitize | Agent 层与 workflow 层分离 | Action 可独立展示和控制 | 视觉+结构化上下文、模式、lint、流式 actions | 需与 HelixFlow 后端语义重新集成 |
| tldraw Workflow | typed ports、binding、依赖执行 | 明确的节点执行图 | 由宿主产品决定 | 节点定义、连线、依赖解析分层 | starter 执行模型本身是演示级 |
| Vercel Workflow Builder | LLM 流式输出 JSONL ops | trigger/action graph | 生成时直接更新画布 | 增量显示、线性/分支布局指引 | prompt 负担重，验证不足，无效行被跳过 |
| OpenSail | 人与 Agent 修改同一结构化配置 | typed graph/config | 结构化 diff 与共享状态 | 单一真相源、round-trip | 面向软件架构，不是媒体管线 |
| n8n | 对话式创建、调试、迭代修改 | 成熟自动化 workflow | 用户补凭据和参数 | 迭代构建、明确人工步骤 | AI builder 内部实现不完全公开 |
| Dify | 可视化节点、运行与 Human Input | workflow/chatflow 共用执行模型 | 原生 Human-in-the-loop | 节点测试、版本、trace、失败路径 | 节点体系比当前 HelixFlow 更复杂 |

## `infinite-canvas` 源码分析

### 整体结构

`infinite-canvas` 将网页画布与本地 Agent 分开：

```text
Codex / Claude Code
  ↓ MCP
Canvas Agent（本地）
  ↓ SSE / HTTP tool request
网页侧边栏
  ↓ apply ops
浏览器画布状态
```

Canvas Agent 默认只监听 `127.0.0.1`，网页通过 token 连接；Agent 通过 MCP 工具读取和修改已连接网页的画布。相关说明见：

- [canvas-agent/README.md](https://github.com/basketikun/infinite-canvas/blob/a0287a5e346e219bd488d084295542213912d816/canvas-agent/README.md)
- [canvas-session.ts](https://github.com/basketikun/infinite-canvas/blob/a0287a5e346e219bd488d084295542213912d816/canvas-agent/src/canvas-session.ts)

### 先读状态，再执行写操作

其 Agent prompt 明确要求修改画布前先调用 `canvas_get_state`。画布读取工具包括：

- `canvas_get_state`
- `canvas_get_selection`
- `canvas_export_snapshot`

返回给模型的节点会经过压缩，避免把大段节点内容无界注入上下文。

可借鉴点：

- HelixFlow 的 Agent turn 应显式读取当前 graph/canvas snapshot、selection、base version 和 gate state。
- Agent 不应根据上一轮自然语言自行假设当前画布仍未变化。
- 大图应提供分层上下文：全图摘要、当前视口、选中节点和按需展开的节点详情。

### 高层工具优先于原始 ops

`infinite-canvas` 不只提供 `canvas_apply_ops`，还提供：

- `canvas_create_generation_flow`
- `canvas_generate_image`
- `canvas_generate_video`
- `canvas_generate_audio`
- `canvas_create_config_node`
- `canvas_connect_nodes`
- `canvas_move_nodes`

高层生成工具最终由确定性代码展开成：

1. 创建 prompt 节点。
2. 创建 generation config 节点。
3. 连接 prompt、参考素材和 config。
4. 选中新节点。
5. 可选地触发生成。

实现见：

- [schemas.ts](https://github.com/basketikun/infinite-canvas/blob/a0287a5e346e219bd488d084295542213912d816/canvas-agent/src/schemas.ts)
- [canvas-session.ts](https://github.com/basketikun/infinite-canvas/blob/a0287a5e346e219bd488d084295542213912d816/canvas-agent/src/canvas-session.ts)

这降低了模型需要同时正确处理节点 ID、坐标、连线、引用语法和运行触发的概率。

对 HelixFlow 的推断：

- 当前 proposal ops 仍然有必要，适合作为后端最小事务格式。
- 但 proposal ops 不应是 Agent 的主要思考接口。
- Agent 应优先输出 `create_media_pipeline`、`replace_model`、`insert_stage`、`connect_typed_ports` 等高层 actions，再由后端编译为 proposal ops。

### 模型配置是正式参数

`infinite-canvas` 的 generation tool schema 正式声明了：

- `model`
- `size`
- `quality`
- `seconds`
- `vquality`
- `count`

生图和视频工作台还分别提供 `workbench_image_get_config` 和 `workbench_video_get_config`，让 Agent 在调用生成前读取可选模型与参数。

可借鉴点：

- 模型选择必须来自后端目录，不能通过节点标题表达。
- `get_config` 或 capability resolution 应先于 create/run。
- 用户给出的品牌名、别名或产品名应被解析为稳定 `model_id`。
- 若无法唯一解析，应要求用户选择，不能选一个默认模型后继续声称使用了指定模型。

### 浏览器持有应用权和撤销权

写操作通过网页侧边栏执行。默认情况下，画布写工具进入待确认状态；用户批准后网页才调用 `applyOps`。应用前保存 snapshot，支持撤销。

实现见：

- [use-agent-bridge.ts](https://github.com/basketikun/infinite-canvas/blob/a0287a5e346e219bd488d084295542213912d816/web/src/pages/canvas/hooks/use-agent-bridge.ts)
- [canvas-local-agent-panel.tsx](https://github.com/basketikun/infinite-canvas/blob/a0287a5e346e219bd488d084295542213912d816/web/src/components/canvas/canvas-local-agent-panel.tsx)

HelixFlow 当前的 proposal + version transaction 比单次浏览器 snapshot undo 更适合持久执行图。建议只借鉴：

- 工具调用卡片。
- 可读的 ops 摘要。
- 写操作确认。
- 流式状态。
- 明确的 undo/rollback 入口。

不建议把浏览器 snapshot 提升为 HelixFlow 的后端 source of truth。

### `infinite-canvas` 的局限

当前通用连接主要表达 `fromNodeId → toNodeId`，没有 HelixFlow 已具备的 typed input/output ports。`canvasOpSchema` 也允许较宽松的 metadata。

因此：

- 它适合图片、视频、文本、参考素材混排的创作画布。
- 它不能直接证明连接在运行时类型兼容。
- 它的高层生成流适合 UX 参考，不应替代 HelixFlow registry、graph validation 和 run compilation。
- 上游 README 已明确提示项目处于开发阶段，不保证历史数据兼容，不应整套视为生产级参考实现。

## tldraw Agent 与 Workflow 架构

### Agent 的“眼睛”

tldraw Agent 同时采集：

- 用户消息。
- 当前 selection。
- 当前视口。
- 用户提供的 bounds 或额外 context。
- 最近用户操作。
- 画布 screenshot。
- 当前视口内的结构化 shape。
- 视口外 shape clusters 摘要。
- 会话历史和 Agent actions。
- canvas lints。

官方说明见 [Agent starter kit](https://tldraw.dev/starter-kits/agent)。

关键价值是同时使用视觉与结构化数据：

- screenshot 帮助模型理解空间、遮挡、视觉分组和布局问题。
- structured shapes 提供稳定 ID、精确字段和可执行对象。

对 HelixFlow 的推断：

- 运行语义必须以结构化 graph 为准。
- screenshot 适合辅助布局、识别拥挤、理解用户视觉关注区域。
- 不应让 screenshot 覆盖 registry、ports 或 params 等机器事实。

### Agent 的“手”

tldraw 为每种 Agent Action 定义独立的：

- schema
- validation
- `sanitizeAction`
- `applyAction`
- chat presentation
- history policy

例如，模型返回不存在的 shape ID 时，sanitizer 可修正或取消操作；新建 ID 可保证唯一；坐标可规范化。

这比“生成一大段 JSON，最后统一解析”更容易：

- 隔离错误。
- 提供局部重试。
- 流式显示进度。
- 为不同风险的 action 设置不同 gate。
- 在聊天中解释每一步实际做了什么。

### Mode 控制能力

tldraw 的 mode 同时决定：

- Agent 可以看见哪些 Prompt Parts。
- Agent 可以调用哪些 Actions。

这比仅在 system prompt 中写“不要做某事”更强。

HelixFlow 可采用类似模式：

| Mode | 可见上下文 | 可用 Actions |
| --- | --- | --- |
| `clarify` | 用户请求、capability resolution 结果 | 提问、列出选项 |
| `plan_workflow` | graph 摘要、catalog、selection | 生成 topology plan，不写 graph |
| `edit_workflow` | base version、相关子图、catalog | graph edit actions |
| `layout_workflow` | 节点尺寸、viewport、selection | move/layout actions |
| `review_workflow` | graph、lint、capability resolution | 评论、修复建议 |
| `run_workflow` | executable graph、provider、cost | estimate/request run |

### Workflow 与 Agent 分层

tldraw Workflow starter 使用：

- 自定义节点定义。
- 输入/输出端口。
- binding 连接。
- 依赖解析。
- 节点执行。

官方文档同时说明 starter 的执行值模型是演示级，可由服务端执行引擎替换。HelixFlow 应借鉴其分层，而不是复制其演示执行器：

```text
Agent actions ≠ workflow execution
Canvas bindings ≠ provider invocation
Visual shape ≠ executable node
```

## Vercel Workflow Builder Template

其 AI 生成端点要求模型按 JSONL 逐行输出 `setName`、`setDescription`、`addNode`、`addEdge`、`removeNode`、`removeEdge` 和 `updateNode`。前端收到 operation 后增量更新 React Flow，因此用户可以立即看到节点和连线出现。

system prompt 还明确区分线性与并行布局，并要求单一 trigger、所有节点从 trigger 可达。源码见：

- [AI generate route](https://github.com/vercel-labs/workflow-builder-template/blob/24fb0fd4524ee10491ea6b73f6df94137ecbf9e1/app/api/ai/generate/route.ts)
- [AI prompt UI](https://github.com/vercel-labs/workflow-builder-template/blob/24fb0fd4524ee10491ea6b73f6df94137ecbf9e1/components/ai-elements/prompt.tsx)

可借鉴的是流式 action、显式 topology 和仅生成必要增量。不能照搬的是其错误处理：生成端点会跳过无效 JSON 行，前端验证主要覆盖 trigger 数量和基本 config，无法证明端口、capability、provider/model、参数、DAG、数据引用、凭据或权限有效。

HelixFlow 的无效 action 应明确失败并记录 validation category，只重试失败 action 或重新规划，不得静默跳过后交付残缺 workflow。

## OpenSail 的共享真相源

OpenSail Architecture Panel 的核心原则是：

> One canvas. One config file. Two authors. Shared state.

人和 Agent 共同读写同一个结构化配置，画布只是该配置的可视化表达。用户拖动或修改节点后，Agent 下一轮可读取新状态；Agent 修改配置后，画布实时反映。

参考：

- [OpenSail README / Architecture Panel](https://github.com/TesslateAI/OpenSail#architecture-panel)

对 HelixFlow 的启示：

- 后端版本化 graph/canvas document 应是共同真相源。
- Agent、手工编辑、导入和 API 修改都应落到同一 transaction/op contract。
- 不应分别维护“Agent graph”和“用户 graph”两套状态。
- visual layout 可以是同一文档的长期字段，但不能与执行语义混淆。

## n8n 与 Dify 的产品流程

### n8n：迭代优于一次生成

n8n 官方建议把 AI Workflow Builder 当作 thought partner：

1. 先生成粗略 workflow map。
2. 运行。
3. 用自然语言修正。
4. 重复迭代。

其公开实践也明确指出：

- 大多数 workflow 仍需要用户补充必要参数和凭据。
- AI builder 完成后应列出人工步骤。
- prompt 应明确 trigger、集成、数据流和输出。
- 不应期待一个超长 prompt 一次生成最终生产 workflow。

参考：[AI Workflow Builder Best Practices](https://blog.n8n.io/ai-workflow-builder-best-practices/)

### Dify：运行控制属于 workflow 本身

Dify 把以下能力作为 workflow 的一等能力：

- Human Input。
- 单节点测试。
- 全流程测试。
- 中间变量与节点输出检查。
- typed error metadata。
- 明确失败路径。
- 版本恢复。
- run trace。

参考：[Dify Workflow Studio](https://dify.ai/workflows)

对 HelixFlow 的推断：

- Agent 创建 graph 只是生命周期的开始。
- 创建后必须提供“缺失配置”“不可运行原因”“建议下一步”。
- 节点级试运行和 capability preflight 比继续增加 prompt 规则更能提高可靠性。
- Human Input 不应只存在于 proposal apply；高风险或关键信息缺失的 run 中也需要暂停与恢复。

## HelixFlow 当前差距

### Agent 直接承担过多底层细节

当前 `CreateWorkflow` 要求 Agent：

- 读取 graph 与多个 catalogs。
- 选择节点。
- 创建 ID。
- 填 params。
- 指定 ports。
- 生成 edges。
- 计算 position。
- 输出完整 proposal wrapper。

这把语义规划、能力解析、图编译和布局同时交给一个 LLM turn。

结果是：

- prompt 越来越长。
- 模型可能产出 schema 合法但产品意图错误的 graph。
- 失败后难以判断是意图、能力、端口、参数还是布局问题。

### 能力目录与执行层不一致

本次 `GPT Image 2 + Seedance 2` 案例暴露出：

- registry 节点 schema 未声明 `model`。
- Atlas gateway 内部却会读取可选 `model`。
- Agent 不能合法写入 model，只能把模型名放进 title。
- 实际执行会采用 gateway 默认模型。

这属于 declaration-execution gap：

```text
用户指定模型
  ↓
Agent 只能改 title
  ↓
graph schema 中没有 model
  ↓
gateway 使用默认 model
  ↓
UI 意图与实际执行不一致
```

### 拓扑没有显式决策

“A + B workflow”可能表示：

- `A → B` 串行。
- `A` 和 `B` 并行。
- `A`、`B` 是可替代候选。
- `A` 生成素材，`B` 只消费 prompt。

当前 prompt 没有要求 Agent先产出 topology decision，也没有在歧义时切换到澄清。

### 缺少 image-to-video 能力

当前目录只有：

- `image.generate`
- `video.text_to_video`

没有 `video.image_to_video`。因此 Agent 无法合法创建：

```text
Prompt
  → GPT Image 2
  → Seedance 2 image-to-video
  → Save Video
```

此前 Agent 将请求降级为两个 prompt 驱动的并行分支。该 graph 在现有 catalog 下可验证，但不符合用户期望。

## 推荐的目标架构

### 一、Intent Contract

在创建 graph 前先形成结构化意图：

```json
{
  "goal": "生成图片并将其作为首帧生成视频",
  "topology": "linear",
  "stages": [
    {
      "requested_model": "gpt-image-2",
      "capability": "text_to_image"
    },
    {
      "requested_model": "seedance-2",
      "capability": "image_to_video"
    }
  ],
  "outputs": ["video"],
  "missing_decisions": []
}
```

规则：

- `topology` 必须为 `linear`、`parallel`、`conditional` 或 `unresolved`。
- 不能根据画布布局反推 topology。
- `unresolved` 必须触发澄清。
- 用户指定的 model 先保留为 request，不直接写入 graph。

### 二、Capability Resolver

确定性解析：

```text
requested model name
  → canonical model_id
  → provider
  → capability
  → node type
  → input/output types
  → parameter schema
```

建议输出：

```json
{
  "requested_model": "seedance-2",
  "status": "unsupported",
  "reason": "selected provider exposes text_to_video but not image_to_video",
  "alternatives": [
    {
      "model_id": "bytedance/seedance-v1.5-pro/text-to-video-fast",
      "capability": "text_to_video"
    }
  ]
}
```

解析失败时不能：

- 用 title 冒充模型。
- 自动换成其他模型但仍显示原名称。
- 把串行改为并行。
- 删除用户要求的阶段后继续。

### 三、高层 Agent Actions

推荐动作：

```text
get_canvas_state
get_selection
resolve_capabilities
plan_workflow_topology
create_media_pipeline
insert_pipeline_stage
replace_stage_model
connect_typed_ports
set_stage_params
layout_subgraph
validate_draft
request_run
```

示例：

```json
{
  "action": "create_media_pipeline",
  "topology": "linear",
  "stages": [
    {
      "capability": "text_to_image",
      "model_id": "openai/gpt-image-2"
    },
    {
      "capability": "image_to_video",
      "model_id": "bytedance/seedance-2"
    }
  ],
  "outputs": ["video"]
}
```

后端负责：

- 分配 node IDs。
- 选择准确 node type。
- 生成 typed edges。
- 填充默认参数。
- 生成 proposal ops。
- 运行 graph validation。
- 调用 layout service。

### 四、Draft 与 Proposal

建议区分：

- `draft actions`: 流式展示，尚未成为 durable graph。
- `proposal`: 已完成能力解析与验证的事务候选。
- `applied version`: 后端事务成功后的新版本。

推荐流程：

```text
stream actions
  → build draft
  → capability validation
  → graph validation
  → layout
  → proposal preview
  → apply
  → version
```

若流式 action 中间失败：

- draft 可以显示局部进度。
- durable graph 不应被部分修改。
- 失败 action 必须显示错误，不得静默跳过。

### 五、Lint 与 Sanitization

推荐 lint：

- model 未解析。
- capability 与 provider 不匹配。
- 必填参数缺失。
- 输入/输出端口类型不兼容。
- 孤立节点。
- 非预期多输出或分支。
- 非法环。
- 节点 title 与实际 model 不一致。
- 输出未连接到 durable output。
- 用户请求 linear，但 graph 出现 fan-out。
- 用户请求单一最终产物，但 graph 出现多个无解释输出。

Sanitizer 只可处理机械错误：

- ID 冲突。
- 坐标无效。
- 已删除 selection。
- 重复 edge。

Sanitizer 不可擅自处理语义错误：

- 替换模型。
- 改变 topology。
- 删除用户要求的阶段。
- 将 `image_to_video` 改成 `text_to_video`。

### 六、Layout 与语义分离

Agent 应输出：

- topology。
- stage order。
- branch groups。
- 用户明确的视觉约束。

布局器应输出：

- `position`。
- 分支间距。
- 防重叠。
- viewport fit。

不建议在 CreateWorkflow prompt 中继续累加大量像素级坐标规则。布局错误不应通过重新调用语义 Agent 修复。

### 七、模式与澄清

建议增加 `clarify` 或等价状态：

```text
用户请求
  ↓
能力与拓扑可唯一确定？
  ├─ 是 → plan/edit
  └─ 否 → clarify
```

典型澄清条件：

- “A + B”无法判断串行还是并行。
- 指定模型不存在或存在多个别名候选。
- capability 存在但 provider 未配置。
- 所需 typed connection 不存在。
- 用户期望单输出，但可行方案只能产生多个独立输出。

## GPT Image 2 → Seedance 2 案例

### 用户期望

推断的目标结构：

```text
Prompt
  ↓
GPT Image 2 文生图
  ↓ image
Seedance 2 图生视频
  ↓ video
Save Video
```

### 实际生成

```text
                 → GPT Image 2 标题的 image.generate → Save Image
Prompt
                 → Seedance 2 标题的 text_to_video → Save Video
```

### 事实

- 生成结果是同一 workflow 中的两个并行分支，不是多个 workflow。
- registry 没有 `image_to_video` node。
- registry node params 没有 `model`。
- gateway 会使用内部默认 model。
- 节点标题不决定执行模型。

### 根因

按优先级：

1. 能力目录缺失 `image_to_video`。
2. registry schema 与 gateway 的 model 参数存在 declaration-execution gap。
3. topology 没有结构化决策。
4. CreateWorkflow prompt 没有要求能力缺失时澄清或失败。

### 正确处理

若能力缺失，应返回：

```text
当前 provider 只提供 text-to-video，没有 image-to-video，因此不能把
GPT Image 2 的输出连接到 Seedance 2。

请选择：
1. 改用支持 image-to-video 的模型。
2. 保留两个并行分支。
3. 只创建图片阶段，稍后再补视频阶段。
```

不得自动选择其中任一项。

## 建议的最小演进顺序

### Phase 1：关闭错误的“伪满足”

- 禁止用 title 表示已选择不可写入 graph 的 model。
- capability 缺失时返回错误或澄清；增加 topology 检查和 title/model 一致性 lint。

### Phase 2：补 Capability Resolver

- 统一 registry、provider catalog 和 gateway params，引入稳定 `model_id`。
- 声明 provider 的 capability 与模态输入输出，增加 `resolve_capabilities` API/action。

### Phase 3：引入高层 Agent Actions

- 首先实现 `create_media_pipeline`，由后端确定性编译为 proposal ops。
- 保留 proposal/version transaction，并在 debug UI 展示展开后的 ops。

### Phase 4：流式 Draft

- action 逐步显示，但 draft 不直接修改 durable graph；验证后才形成 proposal。
- 支持 action 级错误、重试和取消。

### Phase 5：视觉上下文与高级模式

- 为 layout/review 提供 screenshot、viewport、selection、recent actions 和 lint。
- 按 mode 限制上下文和 actions，补节点测试、失败路径与 Human Input。

## 设计原则清单

后续写产品或技术 Spec 时，建议至少检查：

- [ ] 用户指定 model 是否映射到真实 `model_id`。
- [ ] model 是否属于选定 provider。
- [ ] capability 是否真实存在。
- [ ] typed ports 是否兼容。
- [ ] topology 是否显式且符合用户语言。
- [ ] 缺失决策是否进入澄清。
- [ ] Agent 是否优先使用高层 action。
- [ ] 底层 ops 是否由确定性代码生成或严格验证。
- [ ] layout 是否与 graph semantics 分离。
- [ ] 流式更新是否只修改 draft。
- [ ] durable apply 是否仍是事务。
- [ ] 无效 action 是否明确失败。
- [ ] 用户能否看到实际模型、缺失参数和不可运行原因。
- [ ] 是否保留版本、run trace 与人工 gate。

## 来源

### Infinite Canvas

- [项目 README](https://github.com/basketikun/infinite-canvas/blob/a0287a5e346e219bd488d084295542213912d816/README.md)
- [Canvas Agent README](https://github.com/basketikun/infinite-canvas/blob/a0287a5e346e219bd488d084295542213912d816/canvas-agent/README.md)
- [Agent tool schemas](https://github.com/basketikun/infinite-canvas/blob/a0287a5e346e219bd488d084295542213912d816/canvas-agent/src/schemas.ts)
- [Canvas session 与高层工具展开](https://github.com/basketikun/infinite-canvas/blob/a0287a5e346e219bd488d084295542213912d816/canvas-agent/src/canvas-session.ts)
- [Canvas Agent ops](https://github.com/basketikun/infinite-canvas/blob/a0287a5e346e219bd488d084295542213912d816/web/src/lib/canvas/canvas-agent-ops.ts)
- [网页画布桥接与撤销](https://github.com/basketikun/infinite-canvas/blob/a0287a5e346e219bd488d084295542213912d816/web/src/pages/canvas/hooks/use-agent-bridge.ts)
- [Agent 写操作确认](https://github.com/basketikun/infinite-canvas/blob/a0287a5e346e219bd488d084295542213912d816/web/src/components/canvas/canvas-local-agent-panel.tsx)

### tldraw

- [AI integrations](https://tldraw.dev/docs/ai)
- [Agent starter kit](https://tldraw.dev/starter-kits/agent)
- [Workflow starter kit](https://tldraw.dev/starter-kits/workflow)
- [AI-enabled canvas](https://tldraw.dev/use-cases/ai-enabled-canvas)

### Workflow 产品

- [Vercel Workflow Builder Template](https://github.com/vercel-labs/workflow-builder-template)
- [Vercel AI generate route](https://github.com/vercel-labs/workflow-builder-template/blob/24fb0fd4524ee10491ea6b73f6df94137ecbf9e1/app/api/ai/generate/route.ts)
- [OpenSail Architecture Panel](https://github.com/TesslateAI/OpenSail#architecture-panel)
- [n8n AI Workflow Builder Best Practices](https://blog.n8n.io/ai-workflow-builder-best-practices/)
- [Dify Workflow Studio](https://dify.ai/workflows)
