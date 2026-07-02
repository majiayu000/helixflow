# 画布 Agent 技术调研与 Helixflow 架构对照

状态：2026-06-30 研究记录；2026-07-01 追加并补测 Canvas.best / Infinite Canvas。  
语言：中文主文档。  
范围：Higgsfield Canvas 实测、Krea Nodes/Node Agent 官方资料分析、Lovart ChatCanvas 官方资料分析、Canvas.best / Infinite Canvas 真实 Chrome 运行态 + 线上静态 + 开源源码分析、Helixflow 当前代码对照、目标架构建议。

## 0. 先说结论

上一版文档不够好：它是英文，而且 Krea/Lovart 只是简略写了公开资料，没有展开到可执行的架构层面。

这份中文文档做了四件事：

1. 把 Higgsfield 的真实 Chrome 实测结果完整整理成数据流和协议模型。
2. 把 Krea 和 Lovart 从官方页面/文档中能确认的产品与技术模式补齐。
3. 把 Canvas.best / Infinite Canvas 补到真实 Chrome 运行态 + 源码级架构分析。
4. 明确哪些是实测，哪些只是静态 JS 证据，哪些是公开资料，哪些是推断。

最重要的判断：

- Higgsfield 是最接近“生产级协作画布 + 生成任务后端拆分”的样本。
- Krea 更像“节点工作流 + Agent 规划 + 用户审批 + 节点应用封装”的样本。
- Lovart 更像“聊天驱动的设计 Agent + 无限画布 + 可编辑设计资产/图层”的样本。
- Canvas.best / Infinite Canvas 是一个开源 local-first 画布样本：项目、节点、媒体、Agent 会话主要存在浏览器 IndexedDB/localStorage；它有网页内在线 Agent 和本机 Codex Agent 桥接，但不是 Higgsfield 那种远端 canvas-worker/op-log/job-backfill 架构。
- Helixflow 当前不是 Higgsfield/Krea/Lovart 那种完整画布 Agent，而是一个 local-first workflow orchestrator。它有 graph proposal、run service、provider abstraction、cost gate 这些基础，但缺少 canvas-worker、op log、presence/awareness、job result backfill 这些画布系统核心部件。

## 1. 证据等级

本文所有结论按证据等级区分，不把未验证内容写成事实。

| 证据等级 | 含义 |
| --- | --- |
| Chrome 实测 | 在用户真实 Chrome 登录态中打开页面、点击、观察 UI 和 DevTools 数据流。 |
| Runtime probe | 在页面运行时临时包装 `fetch`、`XMLHttpRequest.send`、`WebSocket.prototype.send`，记录请求和 WS send，敏感值已脱敏。 |
| 静态 JS | 下载并搜索生产 JS bundle，证明前端存在这些接口路径或代码分支，但不证明本次运行触发过。 |
| 官方资料 | 厂商官方文档、官网、博客、新闻稿。能证明产品设计和公开能力，不能证明后端真实实现。 |
| 本地代码 | 当前 Helixflow 仓库源代码。 |
| 推断 | 基于前端流量、接口边界和产品行为推断后端设计。推断不等同于后端源码事实。 |

## 2. 本地证据文件

Higgsfield 实测过程中生成过原始事件文件。原始文件含 ticket/session/job/wallet 等敏感上下文，已删除。保留的文件是脱敏后的摘要：

- `/tmp/higgsfield-probe-events-2-sanitized.json`
- `/tmp/hf_probe_install.js`
- `/tmp/higgsfield-bundles-1782803792/page-1bf1c3c938686012.js`

脱敏摘要保留：

- host；
- endpoint；
- HTTP method/status；
- WS message type；
- canvas op 类型；
- node patch 结构；
- redacted 后的 id/ticket/session。

脱敏摘要不保留：

- auth ticket；
- Clerk/session token；
-真实 canvas id；
-真实 asset/job id；
- wallet/subscription balance；
-用户私有 prompt 原文。

## 3. 这几个厂商是否都“完整”写了

按“能证明到什么程度”分：

| 厂商 | 覆盖程度 | 证据等级 | 边界 |
| --- | --- | --- | --- |
| Higgsfield | 最完整。覆盖 canvas list、project route、auth ticket、WS、sync、awareness、node add patch、job detail、wallet、静态 nodes/from-job。 | Chrome 实测 + runtime probe + 静态 JS | 未触发付费生成，所以 job creation 和 nodes/from-job live payload 未抓到。 |
| Krea | 官方产品/架构模式完整。覆盖 Nodes canvas、节点类型、Node Agent、plan/build/validation/cost、Node App Builder。 | 官方资料 | 没有登录态包级实测，不能声称已确认其 WS/API 后端实现。 |
| Lovart | 官方产品/交互模式完整。覆盖 ChatCanvas、Design Agent、MCoT、图层编辑、Touch Edit、Brand Kit、可编辑 composition。 | 官方资料 | 没有登录态包级实测，不能声称已确认其 WS/API 后端实现。 |
| Canvas.best / Infinite Canvas | 运行态 + 代码级完整。覆盖目标 URL、IndexedDB/localStorage、节点 add/select/move、网络加载、在线 Agent、本机 Agent 面板、生成请求源码、WebDAV 同步源码。 | 真实 Chrome 实测 + 线上 HTML/JS + GitHub 源码 | 未实际消耗 API key 触发生成；未配置 WebDAV；未启动本机 canvas-agent。 |

因此：Higgsfield 是完整技术实测；Canvas.best 已补到真实 Chrome 运行态 + 开源源码级确认；Krea/Lovart 是完整公开资料分析 + 架构推断，不是同等级的包级逆向。

## 4. Higgsfield Canvas：实测结论

### 4.1 被测页面

真实 Chrome 登录态中确认：

```text
/canvas
/canvas/{canvasId}
```

观察到：

- `/canvas` 是 canvas list；
-点击卡片进入 `/canvas/{canvasId}`；
-项目路由是真实 canvas editor；
-页面 title 为 Higgsfield Canvas；
-画布上已有媒体/生成节点；
-顶部有 Team Chat、Share；
-左侧有 undo、redo、minimap、zoom；
-底部 toolbar 有 Select、Pan、Draw、Sticky Note、Shape、Text、Arrow、Sticker、Comment、Folder、Add node、Open Higgsie。

### 4.2 Add Node 菜单

实测打开 Add Node 菜单看到：

- Prompt；
- Image Generator；
- Video Generator；
- Voice Generator；
- LLM Assistant；
- New Folder；
- Upload；
- Assets；
- Image Generator；
- Video Generator；
- Voiceover；
- Change Voice；
- Translate；
- Text；
- Sticky Note。

这说明 Higgsfield 的 canvas node 不只是普通图形元素，而是把生成器、媒体资产、文本、comment、folder 等都统一放在 canvas node 体系里。

### 4.3 选中生成节点后的行为

选中一个已有 Image Generation 节点后，右侧/节点详情面板显示：

- Image Generation；
- prompt 输入；
- reference images；
- model；
- aspect ratio；
- resolution；
- batch；
- Regenerate；
- Run pipeline；
-费用/credit。

我没有点击 Regenerate 或 Run pipeline，因为那会触发付费生成。

这次实测观察到的关键点：

- 选中节点本身没有抓到 durable canvas op。
- 选中后触发了 asset detail fetch。
- generation 详情不是从 canvas-worker 取，而是从 `fnf.higgsfield.ai` 取。

### 4.4 Canvas auth ticket

实测进入 canvas editor 后，前端请求：

```http
GET https://canvas-worker.higgsfield.ai/api/flow/{canvasId}/auth
```

响应形状：

```json
{
  "ticket": "<redacted>"
}
```

结论：

- Higgsfield 的 canvas WS 不直接用普通网页 session 作为唯一鉴权；
-编辑器先向 canvas worker 拿短期 ticket；
-ticket 绑定 canvas flow；
-后续 WS 用 ticket 连接。

### 4.5 Canvas WebSocket

实测 WS：

```text
wss://canvas-worker.higgsfield.ai/api/flow/connect/{canvasId}?ticket=<redacted>&format=json
```

初始 sync：

```json
{
  "type": "op",
  "payload": {
    "kind": "sync",
    "lastSeq": 611,
    "pendingOpIds": []
  }
}
```

含义：

- `type=op`：这条消息属于 durable op/sync 通道；
- `kind=sync`：客户端上报自己最后看到的 durable sequence；
- `lastSeq=611`：客户端认为自己已同步到 seq 611；
- `pendingOpIds=[]`：当前没有本地待确认 op。

这就是典型的协作画布 reconnect/sync 协议雏形：

```text
client reconnect
  -> send sync(lastSeq, pendingOpIds)
server
  -> replay seq > lastSeq
  -> ack or dedupe pending op ids
client
  -> reconcile local optimistic state
```

### 4.6 Awareness 通道

同一个 WS 上还发送 awareness：

```json
{
  "type": "awareness",
  "payload": {
    "user": "<redacted>",
    "cursor": "<cursor-or-null>",
    "message": null,
    "drawing": null,
    "drag": null,
    "clientId": "<redacted>"
  }
}
```

脱敏摘要里保留的结构：

```json
{
  "type": "awareness",
  "has_cursor": true,
  "has_drag": false,
  "has_drawing": false,
  "clientId": "<redacted>"
}
```

结论：

- Higgsfield 把 durable op 和 ephemeral awareness 放在同一条 canvas-worker WS 上；
- awareness 用于 cursor、drag、drawing、presence、可能还有临时 message；
- awareness 不应该进入 durable canvas graph；
-这是多人协作画布常见设计：持久状态和临时状态同连接、不同 message type。

### 4.7 Sticky Note 创建 patch

实测操作：

1. 点击 Sticky Note。
2. 点击空白画布。
3. 画布上出现一个空 Sticky Note。

抓到的第一条相关消息：

```json
{
  "type": "op",
  "kind": "sync",
  "lastSeq": 611,
  "pendingOpIds": [
    "<op-id-1>",
    "<op-id-2>",
    "<op-id-3>",
    "<op-id-4>",
    "<op-id-5>",
    "<op-id-6>"
  ]
}
```

随后发送真正 op payload：

```json
{
  "type": "op",
  "kind": "op",
  "top_ops": 6,
  "top_types": {
    "node:add": 1,
    "batch": 5
  }
}
```

其中 `node:add` 摘要：

```json
{
  "node_type": "stickyNote",
  "position": {
    "x": -501.22557878476226,
    "y": 1243.5707500148005
  },
  "data_keys": [
    "text",
    "fontSize",
    "color",
    "authorUsername",
    "input_type",
    "input"
  ],
  "data_summary": {
    "text": "",
    "fontSize": 14,
    "color": "#fef3c7",
    "input_type": "text"
  },
  "style": {
    "width": 200,
    "height": 200
  },
  "zIndex": 15
}
```

5 个 batch 每个包含 3 个 `node:prop`，更新 keys：

```text
data.text
data.richText
data.input.text
```

直接结论：

- 创建 Sticky Note 不是整张 canvas save；
- 是 `node:add` 加一组 `node:prop`；
- node 的视觉属性、业务 data、输入 data 是分层字段；
-客户端有 optimistic outbox，pendingOpIds 能证明这一点；
-批量 batch 支持把多个 field patch 组合成一次高层操作。

### 4.8 Higgsfield durable op 模型

从 Sticky Note 可确认最少存在这些 op 类型：

```text
node:add
node:prop
batch
```

从 UI 和静态 JS 可推测还会有：

```text
node:move
node:remove
edge:add / connection:add
edge:remove / connection:remove
comment:add
comment:update
comment:resolve
folder:add
folder:prop
```

但注意：这次只实测到 `node:add`、`node:prop`、`batch`。其他 op 不能当实测事实。

### 4.9 Generation/job 后端拆分

选中已有 Image Generation 节点时抓到：

```http
GET https://fnf.higgsfield.ai/assets/{assetId}/detail
```

响应摘要：

```json
{
  "job_set_type": "nano_banana_flash",
  "job_set_id": "<redacted>",
  "params_keys": [
    "aspect_ratio",
    "batch_size",
    "height",
    "medias",
    "prompt",
    "reference_elements",
    "resolution",
    "width"
  ],
  "board_ids_count": 0
}
```

还观察到：

```http
GET https://fnf.higgsfield.ai/workspaces/wallet
```

结论：

- canvas worker 不负责直接返回 generation detail；
- generation detail、jobSet、asset、wallet 都在 FNF/job 体系；
- canvas node 上大概率只保存 asset/job 引用和展示状态；
-详情面板需要再向 job/asset 后端查完整信息。

### 4.10 静态 JS 证据：canvas-worker endpoints

Canvas bundle 中确认了这些 route builder：

```text
base: https://canvas-worker.higgsfield.ai
prefix: /api/flow

GET  /api/flow/{id}/auth
GET  /api/flow/{id}/preview
WS   /api/flow/connect/{id}?ticket=...&format=...
GET/POST /api/flow/{id}
POST /api/flow/{id}/run
POST /api/flow/{id}/nodes/{nodeId}/cancel
POST /api/flow/{id}/nodes/from-job
POST /api/flow/{id}/duplicate
templates / canvases / node-types related endpoints
```

`nodes/from-job` 是关键：

```text
/api/flow/{id}/nodes/from-job
```

这说明前端知道一个“从 job 结果回灌到 canvas node”的接口。

但边界必须明确：

- 静态 JS 能证明 endpoint 存在；
-本次没有点击付费 Run；
-所以没有抓到 live `nodes/from-job` payload；
-不能写成“已实测 from-job 回灌”。

### 4.11 静态 JS 证据：job/assets endpoints

生产 JS 中还看到：

```text
GET    /job-sets/{id}
POST   /job-sets/{id}/hide
DELETE /jobs/{id}
POST   /jobs/{id}/view
POST   /jobs/{id}/viewed
POST   /jobs/{id}/track
GET    /jobs/accessible
POST   /jobs/v2/{jobType}
GET    /input-images/{id}?result_type=web_optimized|raw
GET    /input-videos/{id}
```

这支撑一个清晰拆分：

```text
canvas-worker
  -> canvas auth
  -> canvas WS
  -> canvas op log
  -> node placement/state shell
  -> nodes/from-job bridge

fnf/job backend
  -> jobSet
  -> job
  -> asset
  -> wallet/cost
  -> model generation
```

### 4.12 Higgsfield 后端设计推断

这是推断，不是后端源码事实：

```text
Canvas List Service
  -> /canvas list
  -> project metadata

Canvas Worker
  -> validates canvas access
  -> issues short-lived ticket
  -> accepts WS connect(canvasId, ticket)
  -> receives sync(lastSeq, pendingOpIds)
  -> appends durable ops with seq
  -> broadcasts ops to peers
  -> broadcasts awareness without persisting
  -> exposes node control endpoints

Job/FNF Service
  -> creates job/jobSet
  -> tracks job status
  -> persists assets
  -> computes wallet/cost
  -> returns asset detail

Bridge
  -> job finishes
  -> nodes/from-job creates or updates canvas nodes
  -> canvas worker appends node patch ops
```

### 4.13 Higgsfield 未验证项

这些没有实测，不应该写成结论：

- move op payload；
- comment op payload；
- generation job creation request；
- live `nodes/from-job` payload；
- server ack frame；
- conflict resolution；
- binary WS format；
-多用户同时编辑时的 merge 策略。

## 5. Krea Nodes / Node Agent：官方资料完整分析

Krea 这部分不是登录态包级逆向。它是官方文档/页面层面的完整产品和架构模式分析。

主要来源：

- https://docs.krea.ai/user-guide/features/nodes
- https://www.krea.ai/blog/ai-workflow-agent
- https://www.krea.ai/nodes

### 5.1 Krea 的核心定位

Krea Nodes 官方描述是：

```text
在 infinite canvas 上连接 inputs、parameters、outputs，
把 image/video/audio models 串成自定义 node-based workflows。
```

这个定位和 Higgsfield 不完全一样：

- Higgsfield 更像自由画布 + 生成节点 + job 回灌；
- Krea 更像显式 node graph workflow builder；
- Krea 的强项是把复杂 workflow 封装成 app；
- Krea Node Agent 的强项是把自然语言变成可运行 workflow。

### 5.2 Krea node 基本模型

官方文档把每个 node 拆成三类元素：

```text
Inputs
Parameters
Outputs
```

含义：

- Inputs：左侧连接点，可连上游输出，也可手动填值；
- Parameters：节点内部设置，如 strength、resolution、prompt；
- Outputs：右侧输出，流向下游；
- handles 按 data type 区分颜色；
-只允许兼容数据类型连接；
-单个 node 最多 10 个 outgoing connections。

这说明 Krea 的 graph schema 至少需要：

```text
node.id
node.type
node.params
node.input_ports
node.output_ports
edge.from_node
edge.from_port
edge.to_node
edge.to_port
edge.data_type
```

### 5.3 Krea canvas 交互

官方文档确认：

- pan；
- zoom；
- selection box；
- shift multi-select；
- Pan/Select 两种模式；
- section nodes；
- group nodes；
- sticky notes。

这和 Higgsfield 的 toolbar 行为相似，但 Krea 更偏 workflow graph，而不是“任意资产/生成卡片都放进画布”。

### 5.4 Krea node 类型

官方 docs 的 node 分类包括：

- Generate Image；
- Generate Video；
- Edit Image；
- Enhance Image；
- Enhance Video；
- Generate 3D；
- Motion Transfer；
- Lipsync；
- Audio；
- Text Utility；
- Image Utility；
- Video Utility；
- Utility nodes；
- Sticky Note；
- Text Overlay；
- Display/preview 类节点；
- LLM Call。

Krea public `/nodes` 页面还列出工具入口：

- Image；
- Video；
- Nano Banana；
- Realtime；
- Enhancer；
- Edit；
- Video Lipsync；
- Motion Transfer；
- Train；
- 3D Objects；
- Assets；
- Chat；
- Video Restyle；
- Gallery；
- Nodes。

这说明 Krea 的 Nodes 不是单一模型执行器，而是一个“多模型编排层”：

```text
workflow node
  -> model provider/model family
  -> capability
  -> params
  -> output artifact
```

### 5.5 Krea Node Agent 流程

官方 docs 说 Node Agent 的流程是：

```text
用户输入自然语言
  -> agent 读取 canvas
  -> agent 规划 pipeline
  -> agent wiring nodes
  -> agent runs job
```

它读取的 canvas state 包括：

- existing nodes；
- connections；
- earlier run outputs；
- previous style nodes；
- grouped workflows。

这点很关键：Krea Node Agent 不是只看 prompt，而是 canvas-aware。

### 5.6 Krea plan-first 模型

官方 docs 描述：

```text
agent shows plan before touching anything
```

用户可以：

-换模型；
-删除不需要的 stages；
-批准 plan；
-然后 nodes 逐层出现在 canvas；
-每个 node 放置后 wire 到上一个 node；
-用户看到 workflow 实时 assemble。

这对应一个很明确的后端/前端设计：

```text
AgentPlan
  stages[]
  estimated_cost
  proposed_nodes[]
  proposed_edges[]
  validation_report
  user_editable_choices

UserApproval
  -> apply plan
  -> materialize nodes/edges on canvas
```

Helixflow 当前的 proposal/apply 模型和这个方向很接近。

### 5.7 Krea validation/cost

官方资料明确：

- before job runs，agent walks full graph；
-会填 missing parameter；
-会修 incompatible connection；
-会添加 conversion node；
-会显示 per-node compute cost；
-用户确认前不运行。

这说明 Krea 的执行系统至少有：

```text
Graph validator
  -> required params
  -> port type compatibility
  -> conversion node insertion
  -> model availability

Cost estimator
  -> per node compute units
  -> total compute units
  -> plan alternatives
```

这和 Higgsfield 的 wallet/credit UI 是同类设计，但 Krea 是 node workflow 粒度。

### 5.8 Krea cache/downstream rerun

官方 docs 提到：

- cached outputs prevent redundant processing；
-修改一个节点后 only affected nodes rerun；
-编辑链路早期节点会导致更多下游重跑；
-可 clear cache。

这说明 Krea 的 workflow executor 需要 artifact cache：

```text
node_run_cache_key =
  node_type
  model
  params hash
  upstream artifact refs/hash
  workflow version

if unchanged:
  reuse output
else:
  rerun this node and downstream nodes
```

这对 Helixflow 很重要。Helixflow 当前已经有 run steps 和 artifact persistence，但还没有正式的 node-level cache invalidation 策略。

### 5.9 Krea Node App Builder

官方 docs 说 Node App Builder 可以把复杂 workflow 变成简单 app。

步骤：

1. Build workflow。
2. Define inputs and outputs。
3. Design app interface。
4. Publish/share。

可暴露为 app input 的字段：

- text prompts；
- image upload slots；
- dropdown selects；
- sliders；
- number inputs；
- boolean toggles。

隐藏的内容：

- intermediate nodes；
- model switching logic；
- error handling workflows；
- conditional branching。

发布形态：

- public apps；
- private apps；
- embed code；
- API access / Execute a Node App endpoint。

这是一层很重要的产品抽象：

```text
workflow graph
  -> app interface schema
  -> public/private execution surface
  -> API-triggerable workflow runtime
```

Krea 不只是画布，它把画布 workflow 商品化/产品化了。

### 5.10 Krea 后端设计推断

这是推断，不是实测包：

```text
Workflow Store
  -> graph snapshots
  -> node definitions
  -> edges
  -> groups/sections/sticky notes

Agent Planner
  -> reads graph state
  -> reads model/node catalog
  -> creates plan
  -> returns proposed stages
  -> waits for user approval

Graph Validator
  -> required params
  -> port compatibility
  -> conversion nodes

Cost Estimator
  -> per-node compute units
  -> total plan cost

Workflow Executor
  -> topological execution
  -> cache reuse
  -> downstream invalidation
  -> artifact persistence

Node App Runtime
  -> maps app inputs to node params
  -> executes hidden graph
  -> returns selected outputs
  -> supports API endpoint
```

### 5.11 Krea 对 Helixflow 的启发

Helixflow 已经接近 Krea 的几个核心点：

- Agent proposal；
- plan/apply；
- graph ops；
- provider abstraction；
- cost gate；
- run steps；
- artifact persistence。

缺口：

- node type catalog 还太小；
-没有 App Builder；
-没有 workflow cache/downstream-only rerun；
-没有公开 app execution surface；
-没有 Node Agent plan UI 中的 stage edit/swap/drop；
-没有 conversion-node 自动插入；
-没有 per-node cost breakdown UI。

## 6. Lovart ChatCanvas：官方资料完整分析

Lovart 这部分也是官方资料分析，不是登录态包级逆向。

主要来源：

- https://www.lovart.ai/features/infinite-chatcanvas-ai-collaboration
- https://www.lovart.ai/news/lovart-design-agent-public-launch-chatcanvas
- https://www.lovart.ai/blog/how-to-chat-generate-any-design-type-lovart-agent

### 6.1 Lovart 的核心定位

Lovart 把 ChatCanvas 定义为：

```text
real-time infinite workspace
where users collaborate with a Design Agent
```

产品关键词：

- Infinite ChatCanvas；
- AI Design Agent；
- multimodal prompts；
- mood boards；
- reference images；
- generated assets；
- editable composition；
- semantic layer splitting；
- Touch Edit；
- Text Edit；
- Brand Kit；
- high-res export。

Lovart 和 Krea 的不同点：

- Krea 是显式 node graph/workflow；
- Lovart 是 chat-first design canvas；
- Lovart 关注“设计产物可编辑”，不是让用户直接操作 node graph；
- Lovart 的节点/任务编排更可能隐藏在 Agent 内部。

### 6.2 Lovart ChatCanvas 的产品流

官方资料描述的操作链路：

```text
用户在 ChatCanvas 输入 brief
  -> agent 解析 asset type
  -> agent 读取 brand context / Brand Kit
  -> agent 理解 creative direction
  -> 在 infinite canvas 上生成 design composition
  -> composition 不是一张扁平图，而是可编辑元素/图层
  -> 用户通过 Touch Edit / Text Edit / follow-up prompt 精修
  -> export 为目标格式
```

这和 Higgsfield/Krea 最大差别：

- Higgsfield/Krea 的画布节点更像 generation/workflow primitives；
- Lovart 的画布对象更像 design assets / layers / compositions；
- Lovart 的 agent 是 Creative Director 风格，而不是只做 graph planning。

### 6.3 Lovart 的 canvas state

官方资料中可确认 canvas 保存/使用：

- prompts；
- reference images；
- generated assets；
- drafts；
- final edits；
- mood boards；
- brand guidelines；
- color palettes；
- layout context；
- spatial arrangement；
- Brand Kit；
- typography/text layers；
- semantic object layers。

官方页面强调 agent 能理解整个 canvas context，记住 color palettes/style choices，并基于散落在 canvas 上的 references 和 layout 生成变体。

因此 Lovart 的 canvas state 至少需要：

```text
CanvasDocument
  assets[]
  groups/boards[]
  prompts/chat turns[]
  brand kit refs
  style refs
  generated compositions[]
  editable layers[]
  spatial layout
  export targets
```

### 6.4 Lovart 的 editable composition

官方资料多次强调输出不是单张固定图片，而是可编辑 composition：

- text 可编辑；
-元素可点击编辑；
-foreground/background/text layer 可分离；
-可以改颜色、换材质、替换产品图；
-不用整图重生成。

这意味着它不只是保存 `image_url`，还需要保存更结构化的设计结果：

```text
DesignComposition
  base_render_asset
  layers[]
    layer_id
    layer_type: text | image | subject | background | shape | effect
    bbox / mask / transform
    editable properties
    source generation/job refs
  export variants[]
```

实际后端是否这样实现未实测，但从产品能力看，这是合理的数据抽象。

### 6.5 Lovart Touch Edit / Text Edit

官方资料描述：

- click object and tell agent what to change；
-change button color；
-change jacket；
-rewrite text；
-edit typo without regenerating entire image。

这类能力和普通 canvas node patch 不同，它需要：

```text
Hit testing / object selection
  -> identify semantic layer or object mask
  -> create edit instruction
  -> run edit model or compositor
  -> update layer/composition
  -> preserve surrounding context
```

可能的系统边界：

```text
Canvas UI
  -> selected object/layer
  -> edit prompt
Design Agent
  -> interprets edit
Semantic Layer Service
  -> masks/layers
Image Edit Model
  -> produces new layer/render
Composition Store
  -> updates editable composition
```

### 6.6 Lovart MCoT / contextual visual memory

Lovart 官方页面提到 MCoT reasoning engine 和 contextual visual memory。

能确认的产品含义：

- agent 不只看最新 prompt；
-会看整个 canvas；
-记住 palette/style；
-把 references 和 spatial layout 纳入生成；
-让多个 generation 保持一致。

架构上这要求：

```text
Canvas context builder
  -> gather visible assets
  -> gather selected region/layers
  -> gather brand kit
  -> gather prior turns
  -> summarize style/palette/layout intent

Agent planner
  -> produce tasks
  -> choose model/tool
  -> produce design steps
```

Lovart 的“Agent”更接近多工具 creative orchestration，而不是单纯 node DAG executor。

### 6.7 Lovart 多模态任务

官方 launch/news 材料提到：

- images；
- videos；
- audio tracks；
- brand kits；
- 3D renders；
- storyboards；
- animations；
- AR filters。

这说明 Lovart 后台需要一个多模态任务系统：

```text
Task
  type: image | video | audio | brand_kit | 3d | storyboard | ar
  input refs
  brand refs
  canvas context refs
  output assets
  editable composition refs
```

和 Higgsfield 的 generation jobSet 类似，但 Lovart 的结果更偏设计 composition 和 layer graph。

### 6.8 Lovart 后端设计推断

这是推断，不是实测包：

```text
ChatCanvas Service
  -> canvas document
  -> spatial objects
  -> references/assets
  -> editable compositions
  -> collaboration/presence

Design Agent Orchestrator
  -> parses brief
  -> reads canvas context
  -> reads brand kit
  -> plans creative tasks
  -> routes to models/tools

Asset/Composition Service
  -> stores generated assets
  -> stores layers/masks/text objects
  -> stores export variants

Model Job Service
  -> image generation
  -> image edit
  -> video generation
  -> audio generation
  -> 3D generation

Edit Service
  -> Touch Edit
  -> Text Edit
  -> semantic layer splitting
  -> object replacement
```

### 6.9 Lovart 对 Helixflow 的启发

Lovart 对 Helixflow 的启发不是“把所有东西做成节点”，而是：

- chat-first；
- canvas context builder；
-设计资产不一定要暴露为 workflow node；
-输出需要可编辑结构；
-Agent 要能看 canvas 选区、引用图、风格、历史结果；
-编辑不是重跑整条 pipeline，而是 targeted edit。

如果 Helixflow 只做 Krea-style workflow，Lovart 的价值较小。  
如果 Helixflow 要做创意设计 canvas agent，就必须引入：

- canvas selection context；
- asset/layer model；
- partial edit job；
- composition diff；
- Brand Kit / style memory。

## 7. Helixflow 当前代码：它现在到底是什么

当前仓库是 `helixflow`。

README 里的定位：

```text
local-first AI workflow orchestrator
users describe generation tasks
Agent proposes node-graph changes
backend executes approved workflows through model providers
```

技术栈：

```text
Frontend: React 19 + TypeScript + Vite
Backend: Rust + axum + tokio + sqlx + SQLite
Execution: backend RunService
Provider: ModelGateway / provider abstraction
Agent: Codex CLI first
ComfyUI: future optional provider, not v1 core
```

### 7.1 当前 topology

```text
Browser React SPA
  -> REST /api/workspaces/*
  -> WebSocket /ws?workspace_id=...
Local Backend
  -> Workbench
  -> Agent session ctx/ + out/
  -> GraphService
  -> RunService
  -> Provider abstraction
SQLite / local files
```

### 7.2 前端 REST/WS

`web/src/api.ts`：

```text
GET  /api/workspaces/{workspaceId}/state
GET  /api/workspaces
POST /api/workspaces/{workspaceId}/messages
POST /api/workspaces/{workspaceId}/runs
POST /api/workspaces/{workspaceId}/proposals/{proposalId}/apply
POST /api/workspaces/{workspaceId}/proposals/{proposalId}/dismiss
POST /api/workspaces/{workspaceId}/confirmations/{confirmationId}/approve
POST /api/workspaces/{workspaceId}/confirmations/{confirmationId}/hold
WS   /ws?workspace_id={workspaceId}
```

WS 当前只解析 `RunEventEnvelope`。

### 7.3 当前画布 UI

`web/src/components/graph-canvas.tsx`：

- `view` 是本地 React state；
- `mode` 是本地 React state；
- `selected` 是本地 React state；
-pan 只是改变本地 view；
-selected node 只是本地 inspector；
-渲染 graph 或 pendingProposal.previewGraph；
-没有把 add/move/select/comment 转成 canvas op 发 WS。

结论：

当前 Helixflow canvas 是 workflow graph viewer/reviewer，不是多人协作 canvas editor。

### 7.4 当前后端 WS

`crates/server/src/main.rs`：

- `/ws` upgrade；
-订阅 `workbench.events().subscribe()`；
-按 workspace_id 过滤；
-把 `RunEventEnvelope` serialize 后发给 client。

没有：

- client message receive loop；
-canvas op validation；
-awareness fanout；
-op ack；
-op replay；
-pending op reconciliation。

### 7.5 当前 Agent proposal flow

`crates/server/src/workbench.rs`：

```text
send_message
  -> classify chat vs workflow
  -> ChatOnly: agent.answer_chat
  -> empty graph: AgentSkill::CreateWorkflow
  -> existing graph: AgentSkill::ModifyWorkflow
  -> agent.propose_graph_change
  -> write ops.json
  -> write preview.json
  -> create pending proposal
```

`apply_proposal`：

```text
load current context
load proposal
ensure proposal workspace
ensure pending
ensure base_version == current_version
apply ops
create new graph version
resolve proposal
```

这点非常接近 Krea 的 plan-first 模型。

### 7.6 当前 graph ops

`crates/graph/src/lib.rs`：

```text
WorkflowGraph
  schema_version
  nodes: BTreeMap<String, GraphNode>
  edges: Vec<GraphEdge>

GraphNode
  node_type
  title
  params
  pos

GraphEdge
  from: [node_id, port]
  to: [node_id, port]
  edge_type
```

支持 proposal ops：

```text
add_node
remove_node
set_param
add_edge
remove_edge
move_node
```

这是一套 workflow proposal op，不是 live canvas op。  
但它可以映射成未来的 durable canvas op。

### 7.7 当前 run/provider

`RunService`：

-创建 run；
-编译 execution plan；
-按 topological order 执行；
-为每个 step 构造 `ProviderRequest`；
-调用 provider；
-持久化 artifact；
-发 `node.state` event。

`provider.rs`：

- Atlas provider from env；
- Provider trait 包含 `health`、`catalog`、`estimate`、`invoke`、`cancel`；
- Atlas capabilities 包括：
  - chat_completion；
  - image_generate；
  - image_edit；
  - text_to_video；
  - image_to_video。

`registry/src/lib.rs` built-in nodes：

- `input.text`
- `input.image`
- `llm.prompt_writer`
- `image.atlas.generate`
- `video.atlas.text_to_video`
- `video.atlas.image_to_video`
- `output.save`

缺口：

- provider 支持 image_edit，但 registry 还没有 `image.atlas.edit` 节点；
-没有 Krea 那种大量 node catalog；
-没有 node app builder；
-没有 cache/downstream rerun；
-没有 canvas op layer。

## 8. 三家厂商横向对比

| 维度 | Higgsfield | Krea | Lovart | Helixflow 当前 |
| --- | --- | --- | --- | --- |
| 主要形态 | 生成资产画布 | 节点工作流画布 | 设计 Agent 画布 | 本地 workflow orchestrator |
| 画布路由 | `/canvas/{id}` | `/nodes` / workflow | ChatCanvas/canvas | workspace |
| 实时协作 | WS + awareness 已实测 | 官方提到 collaborative canvas editing，但未包级实测 | 官方说 real-time workspace，未包级实测 | 无 |
| 鉴权 | canvas auth ticket 已实测 | 未实测 | 未实测 | 普通本地 REST |
| durable op | `node:add`/`node:prop` 已实测 | 未实测；产品上必有 graph ops | 未实测；产品上必有 canvas/composition ops | proposal ops，非 live ops |
| Agent 模式 | Higgsie/生成 pipeline | Node Agent plan-first | Design Agent chat-first | Codex proposal |
| 执行后端 | FNF job/jobSet/asset | workflow executor / compute units | multimodal design jobs | RunService/provider |
| 成本门槛 | wallet/credit 已实测 | per-node compute cost 官方确认 | pricing/export 官方资料 | cost gate 基础 |
| 输出回灌 | `/nodes/from-job` 静态证据 | node outputs/cached outputs 官方确认 | editable composition/layers 官方确认 | artifacts/run state |
| 封装成 app | 未确认 | Node App Builder 官方确认 | 更偏导出/设计资产 | 无 |
| 图层编辑 | 不明确 | 不核心 | Touch/Edit Elements/Text Edit 核心 | 无 |

## 9. Helixflow 目标架构：兼容三种模式

如果 Helixflow 要做“画布 Agent”，不应该只复制其中一家。建议拆成三层：

```text
Canvas Layer
  Higgsfield-style:
    ticketed WS
    durable ops
    awareness
    op replay
    optimistic outbox

Workflow Layer
  Krea-style:
    node graph
    graph validator
    cost estimator
    workflow executor
    cache/downstream rerun
    app builder

Creative Agent Layer
  Lovart-style:
    chat-first
    canvas context builder
    asset/layer/composition model
    targeted edit
    brand/style memory
```

### 9.1 Canvas service

新增：

```text
POST /api/canvases
GET  /api/canvases
GET  /api/canvases/{id}
POST /api/canvases/{id}/auth
WS   /api/canvases/{id}/connect?ticket=...
```

数据表：

```text
canvases
  id
  workspace_id
  title
  current_seq
  current_snapshot_id
  created_at
  updated_at

canvas_ops
  canvas_id
  seq
  op_id
  client_id
  user_id
  base_seq
  kind
  payload_json
  created_at

canvas_snapshots
  id
  canvas_id
  seq
  document_json
  created_at
```

### 9.2 Canvas op protocol

客户端连接后：

```json
{
  "type": "op",
  "payload": {
    "kind": "sync",
    "lastSeq": 123,
    "pendingOpIds": ["client-op-1", "client-op-2"]
  }
}
```

客户端提交 op：

```json
{
  "type": "op",
  "payload": {
    "kind": "op",
    "opId": "client-op-3",
    "baseSeq": 123,
    "ops": [
      {
        "type": "node:add",
        "node": {
          "id": "node_1",
          "nodeType": "image.generate",
          "position": [120, 240],
          "data": {
            "prompt": "..."
          }
        }
      }
    ]
  }
}
```

服务端 ack：

```json
{
  "type": "op",
  "payload": {
    "kind": "ack",
    "opId": "client-op-3",
    "seq": 124
  }
}
```

服务端广播：

```json
{
  "type": "op",
  "payload": {
    "kind": "remote",
    "seq": 124,
    "opId": "client-op-3",
    "ops": []
  }
}
```

awareness：

```json
{
  "type": "awareness",
  "payload": {
    "clientId": "client_x",
    "cursor": {"x": 1, "y": 2},
    "selection": ["node_1"],
    "drag": null,
    "drawing": null
  }
}
```

### 9.3 Canvas op grammar

最低需要：

```text
node:add
node:remove
node:move
node:prop
edge:add
edge:remove
comment:add
comment:update
comment:resolve
folder:add
folder:prop
batch
```

如果要 Lovart-style：

```text
asset:add
asset:prop
composition:add
composition:layer:add
composition:layer:prop
composition:layer:remove
brand_ref:add
style_ref:add
```

### 9.4 Job backend split

不能让 canvas worker 直接跑模型。应该是：

```text
canvas node action
  -> request job
Run/job service
  -> cost estimate
  -> confirmation if needed
  -> provider invoke
  -> artifact persistence
  -> job status events
nodes/from-job equivalent
  -> append canvas op
  -> create/update result node
```

候选 endpoint：

```http
POST /api/canvases/{canvas_id}/nodes/from-job
```

payload：

```json
{
  "jobId": "job_x",
  "artifactId": "artifact_y",
  "target": {
    "mode": "create_node",
    "position": [120, 240]
  }
}
```

重要原则：

- `nodes/from-job` 不直接改 snapshot；
-它应该 append durable canvas op；
-所有客户端靠 op stream 收到结果。

### 9.5 Krea-style workflow runtime

要补：

```text
node catalog
port type system
graph validator
conversion node insertion
per-node cost estimate
topological executor
artifact cache
downstream invalidation
Node App Builder
Execute App endpoint
```

Helixflow 已有一部分：

- `WorkflowGraph`；
- `ProposalOp`；
- `compile_plan`；
- `RunService`；
- `Provider` trait；
- artifact persistence。

需要升级：

- node catalog 丰富度；
- port type compatibility UI；
- cost breakdown；
- cache key；
- app schema；
- public/private execution permission。

### 9.6 Lovart-style design asset runtime

如果 Helixflow 要进入设计 Agent，而不是只做 workflow，需要补：

```text
Asset
  id
  type
  storage_uri
  source_job_id
  metadata

Composition
  id
  canvas_id
  base_asset_id
  layers[]
  export_targets[]

Layer
  id
  type
  bbox
  transform
  mask_ref
  text
  style
  source_asset_id

BrandKit
  colors
  fonts
  logo_assets
  style_rules
```

Agent context builder：

```text
selected canvas objects
visible surrounding objects
recent chat turns
brand kit
style references
prior outputs
job history
```

这样才能做：

- touch edit；
- text edit；
-局部重绘；
-layer replacement；
-brand-consistent generation。

## 10. 分阶段执行路线

### Phase 1：中文文档和协议冻结

产物：

- 本文档；
- `CanvasOpEnvelope` 草案；
- `AwarenessEvent` 草案；
- `CanvasDocument` 草案；
- `nodes/from-job` 草案。

完成标准：

- 不改现有 runtime；
-只把协议和架构边界写清楚；
-所有未验证项标注清楚。

### Phase 2：本地单用户 canvas op

实现：

- canvas 表；
- op log 表；
- snapshot rebuild；
- REST create/list/get；
-本地 WS；
- `node:add`、`node:move`、`node:prop`。

验证：

- add node 后刷新仍存在；
- move node 后刷新位置正确；
- op log 可 replay；
- snapshot 和 op replay 一致。

### Phase 3：optimistic outbox + reconnect

实现：

- clientId；
- opId；
- pending queue；
- ack；
- duplicate op dedupe；
- reconnect sync(lastSeq, pendingOpIds)。

验证：

-断线期间创建节点；
-重连后不重复；
-服务端 seq 单调；
-多 tab 不丢 op。

### Phase 4：awareness

实现：

- cursor；
- selection；
- drag；
- drawing；
- TTL cleanup；
-不入 DB。

验证：

-两个 tab 互看 cursor；
-断开后 presence 消失；
-awareness 不污染 durable state。

### Phase 5：把现有 proposal ops 映射到 canvas ops

映射：

```text
add_node    -> node:add
remove_node -> node:remove
set_param   -> node:prop
move_node   -> node:move
add_edge    -> edge:add
remove_edge -> edge:remove
```

原则：

- Agent proposal 仍然 plan-first；
-用户 approve 后才 append canvas ops；
-直接手动编辑可以 immediate op；
-两者都进入同一 durable op log。

### Phase 6：job split/from-job

实现：

- canvas generation node；
- request run/job；
- cost confirmation；
- run progress -> canvas node status；
- artifact -> from-job append op；
- result node/card。

验证：

- run 不由 canvas worker 执行；
-取消 job 能更新 node；
-失败 job 能回写 error state；
-成功 job 创建 result node；
-刷新后 result node 存在。

### Phase 7：Krea-style Node App Builder

实现：

- app schema；
- expose node params；
- hidden graph；
- public/private app；
- execute app endpoint；
- per-run cost。

### Phase 8：Lovart-style composition/layer

实现：

- design asset node；
- composition/layer model；
- selected object edit；
- text edit；
- brand kit context；
- partial regeneration。

## 11. 安全规则

绝对不要写入日志：

- auth ticket；
- session token；
- provider key；
- wallet/subscription balance；
-用户私有 prompt；
-原始图片 URL 中的敏感签名参数。

canvas ticket 要求：

-短期有效；
-单 canvas scope；
-绑定 user/workspace；
-可撤销；
-过期后可 refresh；
-不能作为长期 session。

canvas op validation 必须拒绝：

-未知 node type；
-未知 field；
-非法 position；
-无权限 asset/job 引用；
-跨 workspace job；
-comment/sticky note/text 中的 HTML/JS injection；
-富文本 XSS；
-过大的 payload。

## 12. 最终判断

Higgsfield 的可复用技术核心：

```text
ticketed canvas worker
durable incremental op
awareness over WS
generation backend split
nodes/from-job bridge
```

Krea 的可复用技术核心：

```text
canvas-aware Node Agent
plan before build
graph validation
per-node cost
cache/downstream rerun
Node App Builder
```

Lovart 的可复用技术核心：

```text
chat-first canvas agent
canvas context memory
editable composition/layers
touch/text edit
brand/style continuity
multimodal design jobs
```

Helixflow 当前最应该走的路线：

1. 不推翻现有 proposal/run/provider。
2. 先补 Higgsfield-style canvas op layer。
3. 把现有 proposal ops 映射成 durable canvas ops。
4. 把 RunService 结果通过 `nodes/from-job` 类桥接回画布。
5. 再补 Krea-style workflow app builder。
6. 如果产品目标包含设计创作，再补 Lovart-style composition/layer runtime。

## 13. 参考来源

Higgsfield：

- Chrome 实测：用户真实登录态 Chrome。
- Runtime probe：`/tmp/higgsfield-probe-events-2-sanitized.json`。
- Static JS：`/tmp/higgsfield-bundles-1782803792/page-1bf1c3c938686012.js`。

Krea：

- https://docs.krea.ai/user-guide/features/nodes
- https://www.krea.ai/blog/ai-workflow-agent
- https://www.krea.ai/nodes

Lovart：

- https://www.lovart.ai/features/infinite-chatcanvas-ai-collaboration
- https://www.lovart.ai/news/lovart-design-agent-public-launch-chatcanvas
- https://www.lovart.ai/blog/how-to-chat-generate-any-design-type-lovart-agent

Helixflow 本地代码：

- `README.md`
- `SPEC_WORKFLOW_ORCHESTRATOR.md`
- `web/src/api.ts`
- `web/src/components/graph-canvas.tsx`
- `web/src/store.ts`
- `crates/server/src/main.rs`
- `crates/server/src/workbench.rs`
- `crates/agent/src/contract.rs`
- `crates/graph/src/lib.rs`
- `crates/run/src/lib.rs`
- `crates/run/src/cost_gate.rs`
- `crates/server/src/provider.rs`
- `crates/registry/src/lib.rs`

Canvas.best / Infinite Canvas：

- 线上页面：`https://canvas.best/`
- 被问到的画布 URL：`https://canvas.best/canvas/Y_gYqw7Kt1UTN1sy-YYhh`
- 开源仓库：`https://github.com/basketikun/infinite-canvas`
- 本地调研副本：`/tmp/canvas-best-research/infinite-canvas`
- 线上 HTML/JS 抓取：`/tmp/canvas-best-research/share.html`、`/tmp/canvas-best-research/canvas-list.html`、`/tmp/canvas-best-research/chunks/`

## 14. 明确未完成项

如果要把 Krea/Lovart 提升到 Higgsfield 同等级，还需要：

1. 用登录态 Chrome 打开真实 Krea Nodes editor。
2. 安装 probe，创建/移动/连接 node。
3. 抓 Krea 的 graph save、run、cost、agent plan、app builder 流。
4. 用登录态 Chrome 打开 Lovart ChatCanvas。
5. 安装 probe，创建 project、生成 asset、执行 touch edit/text edit。
6. 抓 Lovart 的 canvas object、task/job、layer/composition、export 流。

在这之前，Krea/Lovart 只能称为“官方资料完整分析 + 架构推断”，不能称为“完整后端实测”。

## 15. Canvas.best / Infinite Canvas：代码级分析

### 15.1 本次调研边界

本节最初只有线上静态 + 开源源码确认；2026-07-01 后续补测时，Chrome 插件缓存升级到：

```text
26.623.81905
```

之后 `agent.browsers.get("extension")` 成功，已经可以控制用户真实 Chrome。本文对 Canvas.best 的结论已升级为：

```text
真实 Chrome 运行态 + 线上静态 + 开源源码确认
```

实际完成的证据：

- 用用户真实 Chrome 打开 `https://canvas.best/canvas/Y_gYqw7Kt1UTN1sy-YYhh`；
- 用 CDP 读取真实页面上下文中的 `localStorage`、IndexedDB、DOM、Network 事件；
- 通过 UI 新增 text 节点，观察 IndexedDB diff；
- 通过 UI 选择 text 节点，观察 DOM 状态和 IndexedDB 是否变化；
- 通过 UI 拖动 text 节点，观察 position 持久化；
- 打开网站 Agent / 本机 Agent 面板，观察连接状态和默认配置；
- 抓取 `https://canvas.best/`、`/canvas`、`/canvas/Y_gYqw7Kt1UTN1sy-YYhh` 的线上 HTML；
- 下载线上 Next/Turbopack JS chunks 并搜索关键路径；
- 克隆公开仓库 `basketikun/infinite-canvas`；
- 直接阅读 `web/`、`canvas-agent/`、`plugins/infinite-canvas/` 的源码。

没有完成的证据：

- 没有用用户 API key 点击真实图片/视频/音频生成；
- 没有配置真实 WebDAV 做远端同步；
- 没有启动本地 `canvas-agent` 并让 Codex 通过 MCP 实际操作网页。

所以这一节可以确认 Canvas.best 的画布读写运行态，但生成、WebDAV、本机 agent bridge 仍是源码级确认，不是运行态全链路确认。

#### 15.1.1 真实 Chrome runtime 补测结果

目标 URL：

```text
https://canvas.best/canvas/Y_gYqw7Kt1UTN1sy-YYhh
```

打开后页面没有跳回 `/canvas`，而是直接渲染了本地项目：

```text
title: 无限画布
project title: 无限画布 1
project id: Y_gYqw7Kt1UTN1sy-YYhh
```

这说明该浏览器本地 IndexedDB 里确实存在同 id project。换一个没有这个本地项目的浏览器，源码路径仍然会 `router.replace("/canvas")`。

真实浏览器存储：

```text
localStorage:
  infinite-canvas:ai_config_store
  infinite-canvas:theme_store

IndexedDB:
  database: infinite-canvas
  version: 2
  object store: app_state
  keys:
    infinite-canvas:asset_store
    infinite-canvas:canvas_store
```

`localStorage` 里的 AI config 记录了 BYOK/provider 配置。补测时默认配置为：

```text
channelMode: local
baseUrl: https://api.openai.com
apiKey: ""
imageModel: default::gpt-image-2
videoModel: default::grok-imagine-video
textModel: default::gpt-5.5
audioModel: default::gpt-4o-mini-tts
```

`infinite-canvas:canvas_store` 解析后的真实项目结构：

```json
{
  "id": "Y_gYqw7Kt1UTN1sy-YYhh",
  "title": "无限画布 1",
  "createdAt": "2026-06-30T06:27:08.751Z",
  "updatedAt": "2026-07-01T09:23:47.853Z",
  "backgroundMode": "lines",
  "showImageInfo": false,
  "viewport": {
    "x": -50.87294169227312,
    "y": 50.89609423139659,
    "k": 0.9267296418758806
  },
  "nodes": 4,
  "connections": 1,
  "chatSessions": 1
}
```

真实节点：

| node id | type | title | position | metadata 关键字段 |
| --- | --- | --- | --- | --- |
| `image-1782800840543-4daqa` | `image` | `New Generation` | `x=405.2348,y=637.4286` | `content:""`, `status:"idle"` |
| `text-1782800842842-s6rvg` | `text` | `Note` | `x=187.9292,y=335.9398` | `content:"一只猫咪"`, `prompt:"一只猫咪"`, `fontSize:14`, `status:"success"` |
| `config-1782800851592-lkt79` | `config` | `生成配置` | `x=619.2089,y=-10.0349` | `generationMode:"image"`, `model:"default::gpt-image-2"`, `size:"1:1"`, `count:3`, `status:"idle"` |
| `video-1782800877701-c0oeg` | `video` | `Video` | `x=669.6986,y=304.9247` | `content:""`, `status:"idle"` |

真实连线：

```json
{
  "id": "cRKGx9skqTHYOOqlfx0Ys",
  "fromNodeId": "text-1782800842842-s6rvg",
  "toNodeId": "config-1782800851592-lkt79"
}
```

真实 chat session：

```json
{
  "id": "ZL7Iaur68H1iGNL_5XVbi",
  "title": "新对话",
  "messageCount": 0
}
```

DOM 节点有稳定运行态属性：

```html
data-node-id="text-1782800842842-s6rvg"
class="node-element absolute flex select-none ..."
style="transform: translate(187.929px, 335.94px); width: 340px; height: 240px; ..."
```

这说明 Canvas.best 的前端不是 Canvas 2D 直接绘制所有节点，而是 DOM absolute nodes + transform viewport。

UI 新增 text 节点测试：

```text
点击 toolbar 的“文本”按钮
nodeCount: 4 -> 5
new id: text-1782897947555-dkd95
metadata:
  content: ""
  fontSize: 14
  status: "idle"
position:
  x: 700.6670
  y: 223.7938
```

该动作没有产生 `Network.requestWillBeSent` / WebSocket 事件；只写入 IndexedDB 的 `infinite-canvas:canvas_store` snapshot。测试节点已清理回 4 个节点。

UI 选择 text 节点测试：

```text
点击 text-1782800842842-s6rvg
IndexedDB rawLength 不变
project.updatedAt 不变
node.position 不变
DOM class: z-10 -> z-50
```

结论：选择状态是 React/UI 内存状态，不进入持久项目 JSON。

UI 拖动 text 节点测试：

```text
before position:
  x: 187.9291753205523
  y: 335.9398494074465

after position:
  x: 252.67297684453115
  y: 384.49770055043064

updatedAt:
  2026-07-01T09:27:42.208Z
  -> 2026-07-01T09:28:48.932Z
```

结论：move 操作直接修改节点 `position` 并写回 `canvas_store` snapshot，不是远端 op patch。拖动测试后已把节点位置恢复到原始坐标。

页面 reload 网络请求：

- `GET https://canvas.best/canvas/Y_gYqw7Kt1UTN1sy-YYhh`；
- 多个 `/_next/static/chunks/*.js`；
- `https://static.cloudflareinsights.com/beacon...`；
- `https://canvas.best/cdn-cgi/rum?`；
- `https://raw.githubusercontent.com/basketikun/infinite-canvas/main/VERSION`；
- icon/favicon 请求；
- Chrome 扩展 content script 请求。

没有观察到：

- WebSocket；
- `/api/canvas/...`；
- canvas auth ticket；
- canvas worker；
- `nodes/from-job`；
- first-party generation job backend。

Agent 面板运行态：

```text
默认模式: 网站
模型: gpt-5.5 · 默认渠道
工具确认: checked
```

本机模式运行态：

```text
状态: 未连接
默认 Local URL: http://127.0.0.1:17371
需要 Connect token
启动命令提示: canvas-agent
安装命令提示: npm i -g @basketikun/canvas-agent
```

未填 token 时没有连接本地服务，也没有触发 `/events`、`/canvas/state`、`/canvas/result`。

清理说明：

- 新增测试 text 节点已删除；
- 拖动过的 text 节点已恢复原位置；
- 因为 UI undo 在 reload 后不可用，清理/恢复使用 CDP 在 IndexedDB 中直接写回原始项目 JSON；
- 没有触发生成；
- 没有填入或发送 API key；
- 没有配置 WebDAV；
- 没有启动本机 `canvas-agent`。

### 15.2 产品定位

Canvas.best 线上页和 GitHub 仓库都指向同一个开源项目：

```text
https://github.com/basketikun/infinite-canvas
```

当前版本：

```text
v0.4.0
```

它的定位是开源无限画布工作台：

- 多画布项目；
- 图片、文本、配置、视频、音频节点；
- 节点拖拽、缩放、连线、小地图、撤销重做；
- 图片/视频/音频生成；
- 提示词库；
- 素材库；
- WebDAV 同步；
- 网页内在线 Agent；
- 本机 Codex/Claude Agent 桥接；
- Codex App 插件。

这和 Higgsfield 的最大差别：

```text
Higgsfield = SaaS canvas worker + auth ticket + WebSocket op log + generation job backend
Canvas.best = local-first browser app + user BYOK provider API + optional local agent bridge
```

### 15.3 路由行为

源码路由：

```text
web/src/app/(user)/canvas/page.tsx
web/src/app/(user)/canvas/[id]/page.tsx
web/src/app/(user)/canvas/[id]/canvas-client-page.tsx
```

`/canvas` 是本地画布库页面：

- 等 `useCanvasStore` hydrate；
- 从本地 `projects` 数组渲染卡片；
- `createProject()` 后 `router.push('/canvas/{id}')`；
- 导入/导出是 zip，本质也是本地项目 JSON + 媒体 Blob。

`/canvas/{id}` 是客户端编辑器：

- Next route 参数提供 `projectId`；
- `openProject(projectId)` 从本地 store 查项目；
- 如果找不到项目，执行 `router.replace('/canvas')`；
- 找到项目后，把 `project.nodes/connections/chatSessions/viewport` hydrate 到 React state。

对用户给的链接：

```text
https://canvas.best/canvas/Y_gYqw7Kt1UTN1sy-YYhh
```

线上 HTML 里能确认：

- 服务端返回 editor shell 和 route params；
- route payload 包含 `["id","Y_gYqw7Kt1UTN1sy-YYhh"]`；
- 没有任何该画布的项目 JSON；
- 没有 `projects` 文档 payload；
- 没有远端 canvas document API 调用证据。

因此，这个 URL 不是 Higgsfield 那种“远端项目 ID”。它只会在当前浏览器本地 store 里找同名 project id。除非本浏览器曾经创建/导入过这个 id，否则打开后会回到 `/canvas`。

### 15.4 前端存储模型

核心 store：

```text
web/src/app/(user)/canvas/stores/use-canvas-store.ts
web/src/lib/localforage-storage.ts
web/src/services/image-storage.ts
web/src/services/file-storage.ts
```

localForage 配置：

```ts
localforage.config({
  name: "infinite-canvas",
  storeName: "app_state",
});
```

画布项目 key：

```text
infinite-canvas:canvas_store
```

项目类型：

```ts
type CanvasProject = {
  id: string;
  title: string;
  createdAt: string;
  updatedAt: string;
  nodes: CanvasNodeData[];
  connections: CanvasConnection[];
  chatSessions: CanvasAssistantSession[];
  activeChatId: string | null;
  backgroundMode: "lines" | "dots" | "blank";
  showImageInfo: boolean;
  viewport: { x: number; y: number; k: number };
};
```

媒体 Blob 不直接长期存在项目 JSON 里，而是分开存：

```text
database: infinite-canvas
store: image_files
key: image:{nanoid}

database: infinite-canvas
store: media_files
key: video:{nanoid} / audio:{nanoid} / file:{nanoid}
```

项目 JSON 中只保存：

- `storageKey`；
- 当前展示用 `content`，通常是本次 session 的 `blob:` URL；
- `mimeType`；
- `bytes`；
- `naturalWidth/naturalHeight`；
- prompt / generation metadata。

打开画布时会 hydrate：

- 有 `storageKey`：从 IndexedDB 读 Blob，重新生成 `blob:` URL；
- 旧数据只有 base64：迁移到 `image_files`，补 `storageKey`；
- 助手消息、素材库也遵循同一套补水逻辑。

这说明 Canvas.best 的核心设计是 local-first，而不是 server-authoritative canvas document。

### 15.5 节点和连线结构

源码：

```text
web/src/app/(user)/canvas/types.ts
web/src/app/(user)/canvas/constants.ts
```

节点类型：

```ts
enum CanvasNodeType {
  Image = "image",
  Text = "text",
  Config = "config",
  Video = "video",
  Audio = "audio",
}
```

节点结构：

```ts
type CanvasNodeData = {
  id: string;
  type: CanvasNodeType;
  title: string;
  position: { x: number; y: number };
  width: number;
  height: number;
  metadata?: CanvasNodeMetadata;
};
```

连线结构：

```ts
type CanvasConnection = {
  id: string;
  fromNodeId: string;
  toNodeId: string;
};
```

metadata 覆盖的业务字段很多：

```ts
type CanvasNodeMetadata = {
  content?: string;
  composerContent?: string;
  prompt?: string;
  status?: "idle" | "success" | "loading" | "error";
  errorDetails?: string;
  fontSize?: number;
  generationMode?: "text" | "image" | "video" | "audio";
  generationType?: "generation" | "edit";
  model?: string;
  size?: string;
  quality?: string;
  count?: number;
  seconds?: string;
  vquality?: string;
  generateAudio?: string;
  watermark?: string;
  audioVoice?: string;
  audioFormat?: string;
  audioSpeed?: string;
  audioInstructions?: string;
  references?: string[];
  naturalWidth?: number;
  naturalHeight?: number;
  freeResize?: boolean;
  isBatchRoot?: boolean;
  batchRootId?: string;
  batchChildIds?: string[];
  batchUsesReferenceImages?: boolean;
  primaryImageId?: string;
  imageBatchExpanded?: boolean;
  storageKey?: string;
  mimeType?: string;
  bytes?: number;
  durationMs?: number;
};
```

默认节点尺寸：

```text
image  340 x 240
text   340 x 240
config 340 x 240
video  420 x 236
audio  340 x 120
```

这里没有单独的 `CanvasOpEnvelope` durable log，也没有 CRDT/OT。节点和连线数组就是当前 project state。

### 15.6 编辑器内状态流

编辑器启动：

```text
useCanvasStore.hydrated
-> openProject(projectId)
-> hydrateCanvasImages(project.nodes)
-> hydrateAssistantImages(project.chatSessions)
-> setNodes / setConnections / setChatSessions / setViewport
```

编辑期间：

- React state 保存当前 `nodes`、`connections`、`selectedNodeIds`、`viewport`；
- 添加节点：`createCanvasNode()` 后 `setNodes([...prev, newNode])`；
- 移动节点：pointer move 中直接 `setNodes(map position)`；
- 连线：`setConnections([...prev, connection])`；
- 删除：过滤 nodes，并同步过滤相关 connections；
- 选择：只改组件 state，不进入 durable project；
- history：`historyRef` 在内存里维护 undo/redo，不是独立持久化 op log。

持久化：

```text
useEffect -> updateProject(projectId, { nodes, connections, chatSessions, activeChatId, backgroundMode, showImageInfo })
viewport -> 500ms debounce -> updateProject(projectId, { viewport })
store persist -> localForage setItem -> 400ms debounce
```

这条链路是“本地 state snapshot 持久化”，不是“增量 patch over websocket”。

### 15.7 Agent op 模型

源码：

```text
web/src/app/(user)/canvas/utils/canvas-agent-ops.ts
```

Agent 可返回的 op：

```ts
type CanvasAgentOp =
  | { type: "add_node"; ... }
  | { type: "update_node"; id: string; patch?: Partial<CanvasNodeData>; metadata?: CanvasNodeMetadata }
  | { type: "delete_node"; id?: string; ids?: string[]; nodeType?: CanvasNodeType }
  | { type: "delete_connections"; id?: string; ids?: string[]; all?: boolean }
  | { type: "connect_nodes"; id?: string; fromNodeId: string; toNodeId: string }
  | { type: "set_viewport"; viewport: ViewportTransform }
  | { type: "select_nodes"; ids: string[] }
  | { type: "run_generation"; nodeId: string; mode?: "text" | "image" | "video" | "audio"; prompt?: string };
```

`applyCanvasAgentOps()` 的行为：

- `add_node`：按 node type 填默认 spec，追加节点；
- `update_node`：merge 基础 patch 和 metadata；
- `delete_node`：删除节点并清理相关连线；
- `delete_connections`：按 id 或 all 删除；
- `connect_nodes`：检查节点存在、避免重复连线；
- `set_viewport`：改 viewport；
- `select_nodes`：只保留存在的 id；
- `run_generation`：不直接改 state，由 editor 里 `generateNodeRef.current` 触发生成。

这套 op 是网页内 Agent/本机 Agent 的命令协议，不是服务端同步协议。

### 15.8 生成链路

源码：

```text
web/src/app/(user)/canvas/[id]/canvas-client-page.tsx
web/src/app/(user)/canvas/components/canvas-node-generation.ts
web/src/services/api/image.ts
web/src/services/api/video.ts
web/src/services/api/audio.ts
```

生成上下文：

- 从目标节点的上游 `connections` 找资源节点；
- 文本节点提供文本；
- 图片节点提供 reference image；
- 视频节点提供 reference video；
- 音频节点提供 reference audio；
- config 节点的 `composerContent` 支持 `@[node:{id}]` token；
- 生成前会把本地 Blob 转成 provider 需要的 data URL。

图片生成：

```text
requestGeneration()
  OpenAI format -> POST {baseUrl}/images/generations
  Gemini format -> POST {baseUrl}/v1beta/models/{model}:generateContent

requestEdit()
  OpenAI format -> POST {baseUrl}/images/edits FormData(image/mask)
  Gemini format -> generateContent with image parts
```

视频生成：

```text
createVideoGenerationTask()
  OpenAI format -> POST {baseUrl}/videos
  poll -> GET {baseUrl}/videos/{id}
  content -> GET {baseUrl}/videos/{id}/content

Seedance/Volc format -> POST {baseUrl}/contents/generations/tasks
  poll -> GET {baseUrl}/contents/generations/tasks/{id}
```

音频生成：

```text
POST {baseUrl}/audio/speech
```

结果回写：

1. 先在画布上创建目标节点或子节点，`status: "loading"`；
2. 浏览器直接请求用户配置的 provider API；
3. provider 返回图片/video/audio；
4. `uploadImage()` 或 `uploadMediaFile()` 把 Blob 写入 IndexedDB；
5. `setNodes()` 把 `content/storageKey/status/prompt/model/size/...` 回填到节点；
6. `updateProject()` 持久化整个项目 state。

因此，Canvas.best 没有独立的 first-party generation job backend，也没有 Higgsfield 那种 `nodes/from-job` 回灌接口。

### 15.9 在线 Agent

源码：

```text
web/src/app/(user)/canvas/components/canvas-assistant-panel.tsx
```

在线 Agent 是网页内执行：

- 用当前网页配置的 text model；
- 把 compact canvas snapshot 放进模型上下文；
- 工具定义在前端代码里；
- 首轮强制 tool call；
- read-only 工具可直接执行；
- writable 工具如果 `confirmTools` 开启，会等待用户批准；
- 批准后在浏览器内 `onlineToolToOps()` -> `onApplyOps()`；
- 需要生成时，工具返回 `run_generation`，再触发本地网页的生成链路。

核心工具：

```text
canvas_get_state
canvas_get_selection
canvas_export_snapshot
canvas_apply_ops
canvas_create_node
canvas_create_text_node
canvas_create_text_nodes
canvas_create_config_node
canvas_create_image_prompt_flow
canvas_create_generation_flow
canvas_generate_text
canvas_generate_image
canvas_generate_video
canvas_generate_audio
canvas_update_node
canvas_update_node_text
canvas_move_nodes
canvas_resize_node
canvas_delete_nodes
canvas_connect_nodes
canvas_select_nodes
canvas_set_viewport
canvas_run_generation
```

这条链路没有远端 Agent server。模型 API 仍由浏览器直接调用用户配置的 provider endpoint。

### 15.10 本机 Codex Agent 桥

源码：

```text
canvas-agent/src/http-server.ts
canvas-agent/src/canvas-session.ts
canvas-agent/src/schemas.ts
canvas-agent/src/mcp-server.ts
canvas-agent/src/agents.ts
canvas-agent/src/config.ts
web/src/app/(user)/canvas/components/canvas-local-agent-panel.tsx
plugins/infinite-canvas/skills/open-canvas/SKILL.md
plugins/infinite-canvas/skills/canvas/SKILL.md
```

本机 agent 默认监听：

```text
http://127.0.0.1:17371
```

本机配置：

```text
~/.infinite-canvas/canvas-agent.json
```

token：

```ts
crypto.randomBytes(18).toString("hex")
```

HTTP/SSE API：

```text
GET  /health
GET  /config
GET  /events?token=...&clientId=...
POST /canvas/state
POST /canvas/result
POST /api/tools
GET  /agent/codex/workspace
GET  /agent/codex/threads
POST /agent/codex/threads/new
GET  /agent/codex/threads/:threadId
POST /agent/codex/threads/:threadId/resume
POST /agent/codex/threads/:threadId/delete
POST /agent/codex/turn
POST /agent/claude/turn
```

本机 agent 的同步方式：

1. 网页打开 SSE `/events`；
2. SSE `hello` 后，网页 `POST /canvas/state` 上传当前 canvas snapshot；
3. 网页每 300ms debounce 把新 snapshot 发给 agent；
4. Codex/MCP 调工具时，请求进入 `POST /api/tools`；
5. `CanvasSession.callTool()` 把高级工具转换为 `canvas_apply_ops`；
6. agent 通过 SSE `tool_call` 发给网页；
7. 网页弹确认或自动执行；
8. 网页 `onApplyOps()` 应用到本地 state；
9. 网页 `POST /canvas/result` 返回结果；
10. agent resolve pending promise；
11. Codex 继续下一轮。

这是“本机 agent bridge”，不是多人实时协作 worker：

- `CanvasSession` 只在内存保存最新 snapshot；
- 没有 durable op seq；
- 没有跨浏览器同步；
- 没有 presence/awareness；
- 网页仍是最终执行方。

Codex 集成：

- agent 通过 `codex app-server --stdio` 管理 Codex thread；
- 每个 canvas id 可有独立 workspace；
- 默认 workspace 在 `~/.infinite-canvas/codex-workspaces/{canvasId}`；
- MCP server 注册同一批 `canvas_*` 工具；
- Codex 工具调用最终还是 POST 到本机 `canvas-agent`。

### 15.11 WebDAV 同步

源码：

```text
web/src/services/app-sync.ts
web/src/services/webdav-sync.ts
web/src/app/webdav-proxy/route.ts
```

同步域：

```text
canvas
assets
image-workbench
video-workbench
```

每个域上传一个 manifest：

```ts
type DomainManifest<T> = {
  app: "infinite-canvas";
  version: 1;
  domain: "canvas" | "assets" | "image-workbench" | "video-workbench";
  exportedAt: string;
  data: T;
  files: Array<{
    storageKey: string;
    path: string;
    mimeType: string;
    bytes: number;
  }>;
};
```

同步策略：

- 先读远端 manifest；
- 读本地 store；
- `mergeById()` 按 `updatedAt` 或 `createdAt` 合并；
- 下载本地缺失的媒体 Blob；
- 上传新增或 size 变化的 Blob；
- 上传新的 manifest。

WebDAV 不是 Canvas.best 的中心后端，而是用户自带远端备份/同步存储。

部署风险：

- `/webdav-proxy` 允许客户端 header 指定 `x-webdav-target`；
- 代码只限制协议为 `http:` / `https:`；
- 如果公开部署，需要加鉴权、目标白名单或仅允许可信用户访问，否则可能成为开放代理。

### 15.12 Next.js 后端边界

Canvas.best 自己的 Next API 很少：

```text
GET  /api/prompts
POST /webdav-proxy
```

`/api/prompts`：

- 从多个 GitHub raw prompt repository 拉取 markdown/json；
- 内存缓存 1 小时；
- 返回 prompt list/tags/categories。

`/webdav-proxy`：

- 代理 WebDAV 请求；
- 用于绕过用户 WebDAV CORS；
- 不保存画布文档。

没有看到：

- `/api/canvas/{id}`；
- `/api/canvas/{id}/auth`；
- `/api/flow/connect`；
- canvas websocket；
- server-side canvas op log；
- server-side generation queue；
- jobSet / generation 后端；
- nodes/from-job。

### 15.13 和 Higgsfield 的关键差异

| 维度 | Higgsfield | Canvas.best / Infinite Canvas |
| --- | --- | --- |
| 项目列表 | 远端 `/canvas` 列表 | 本地 `projects` 数组 |
| 项目 ID | SaaS canvas id | local project id |
| 鉴权 | canvas auth ticket | 本地 store；local agent token 只保护本机 agent |
| 实时同步 | WebSocket canvas worker | 无 canvas sync WS |
| 增量 patch | durable op + seq | React state snapshot + localForage |
| presence | WS awareness | 无多人 presence |
| 生成任务 | 远端 job/jobSet/generation | 浏览器直接请求用户 provider API |
| 结果回灌 | `/nodes/from-job` 静态证据 | `setNodes()` 本地回填 |
| Agent | SaaS canvas/product agent | 网页内在线 Agent + 本机 Codex bridge |
| 后端存储 | SaaS DB/worker 推断 | 用户浏览器 IndexedDB；可选 WebDAV |

结论：

Canvas.best 是很好的“开源画布 Agent 工程样本”，尤其适合学习：

- local-first 数据结构；
- BYOK provider 调用；
- Agent tools -> canvas ops；
- Codex/MCP -> 本机 HTTP/SSE -> 网页执行；
- WebDAV manifest 同步。

但它不是 Higgsfield 式生产级 SaaS 协作画布后端样本。

### 15.14 对 Helixflow 的启发

可以直接借鉴：

1. `CanvasAgentOp` 这种工具协议层：Agent 不直接模拟鼠标，而是返回 typed ops。
2. `run_generation` 只作为 op，不在 Agent 层直接塞结果：生成仍走统一 generation handler。
3. `composerContent` + `@[node:{id}]`：用显式引用 token 表达配置节点输入顺序。
4. 媒体 Blob 和项目 JSON 分离：artifact 元数据进节点，二进制文件单独存。
5. 本机 agent bridge：网页把 snapshot 发给本机 agent，本机 Codex/MCP 调工具，网页负责最终 apply。
6. `confirmTools`：写操作可让用户批准，降低 Agent 误改画布风险。

不应该照搬：

1. 只存 snapshot、不存 op log：Helixflow 如果要多人/可审计/可恢复，应补 durable op log。
2. 浏览器直接存 API key：产品化 SaaS 应改成 server-side secret 或用户本地模式明确隔离。
3. WebDAV proxy 无目标白名单：公有部署需补安全边界。
4. 没有 provider-side job abstraction：Helixflow 已有 RunService，应保留 job/run 层，而不是让浏览器直接打 provider。

推荐融合路线：

```text
Helixflow existing proposal/run/provider
  + Canvas.best style typed agent ops
  + Higgsfield style durable op log / canvas worker / awareness
  + Helixflow RunService as generation backend
  + nodes/from-job style result backfill
```

### 15.15 Canvas.best 未完成项

Canvas.best 已完成真实 Chrome 运行态补测，但还没有做到 Higgsfield 那种“生成任务后端 live payload + worker protocol”级别。剩余项：

1. 配置一个测试 provider key，触发一次低成本生成，抓浏览器 network；
2. 记录生成过程中的 request body、stream/poll 行为、错误处理和节点回填；
3. 创建 image/video/audio 节点并上传真实媒体，记录 `image_files` / `media_files` object store；
4. 配置测试 WebDAV，抓 manifest 和 files 上传/下载；
5. 启动 `npx -y @basketikun/canvas-agent`，连网页，抓 SSE `/events`、`/canvas/state`、`/canvas/result`；
6. 通过 MCP 调 `canvas_create_text_node`、`canvas_apply_ops`、`canvas_run_generation`；
7. 对照线上 chunks hash 和 GitHub commit，确认线上部署是否完全等于当前 GitHub `main`。

在这些完成前，本文对 Canvas.best 的结论是“画布读写 runtime 已实测；生成/WebDAV/本机 agent bridge 仍是源码级确认”。
