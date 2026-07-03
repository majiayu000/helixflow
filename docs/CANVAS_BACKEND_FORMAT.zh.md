# Helixflow 后端画布格式与落地差距

本文基于当前仓库代码和已完成的画布产品实测调研，定义 Helixflow 后端应该保存的画布格式，并列出本地实现距离该目标的差距。

结论先行：

- 后端 source of truth 不应该继续只用 `WorkflowGraph`。`WorkflowGraph` 应保留为执行投影，只表达可运行节点和可运行边。
- 后端 source of truth 应该是 `CanvasDocument` snapshot 加 `CanvasOpEnvelope` 增量日志。
- 生成任务不属于 canvas worker 本身。画布只发起 `run.request`，现有 `runs` / `run_steps` / `artifacts` 继续负责执行和产物，产物再通过 `artifact.attach` 或 `node.patch` 回灌到画布节点。
- 选择框、hover、当前视口、光标属于 presence，不进入长期版本历史；节点、边、参数、评论、产物引用、运行状态才进入 durable op。

## 证据边界

### 已实测的外部事实

Canvas.best 目标页面 `https://canvas.best/canvas/Y_gYqw7Kt1UTN1sy-YYhh` 已通过真实 Chrome 打开使用：

- 页面项目名为 `无限画布 1`，canvas id 为 `Y_gYqw7Kt1UTN1sy-YYhh`。
- IndexedDB 为 `infinite-canvas`，store 为 `app_state`，关键 key 包括 `infinite-canvas:canvas_store`。
- 实测项目包含 4 个节点、1 条 connection、1 个 chat session。
- 添加 text 节点只写 IndexedDB snapshot，没有 WebSocket 或远端 canvas API。
- select 节点只改变 DOM class / z-index，不写 IndexedDB。
- move 节点修改 IndexedDB 中节点 `position` 和 `updatedAt`，没有远端 op patch。
- 页面 reload 期间没有发现 WebSocket、`/api/canvas`、canvas worker、`nodes/from-job` 或一等公民 job backend。

这说明 Canvas.best 更接近 local-first snapshot 模型，不能直接作为 Helixflow 多端协作/后端权威画布的完整参考。

Higgsfield 类 canvas agent 的已归纳流程是：

1. `/canvas` 是 canvas list。
2. 点击卡片后路由变成 `/canvas/{canvasId}`。
3. 编辑器获取 canvas auth ticket。
4. 建立 WebSocket 到 canvas worker。
5. add / move / select / comment / node 操作变成增量 patch，通过 WebSocket 同步。
6. 生成任务转到 job / jobSet / generation 后端。
7. 生成结果再通过 nodes/from-job 回灌成 canvas 节点。

Helixflow 应采用 Higgsfield 式的后端权威 op-log 和 job 回灌边界，但不需要照搬它的远端 worker 拆分；仓库已经有 workspace、version、proposal、run、artifact 层，可以在这些基础上补 canvas 层。

### 当前仓库事实

当前 `crates/graph/src/lib.rs` 中的核心结构是：

```rust
pub struct WorkflowGraph {
    pub schema_version: u32,
    pub nodes: BTreeMap<String, GraphNode>,
    pub edges: Vec<GraphEdge>,
}

pub struct GraphNode {
    pub node_type: String,
    pub title: String,
    pub params: Value,
    pub pos: [f32; 2],
}

pub struct GraphEdge {
    pub from: [String; 2],
    pub to: [String; 2],
    pub edge_type: String,
}
```

这适合执行编排，不适合作为无限画布完整文档，因为它没有：

- 非执行节点：text、image、video、comment、group、config、artifact card。
- 节点尺寸、z-index、锁定、折叠、颜色等 UI 状态。
- 评论线程、产物引用、运行状态回灌、presence。
- 增量 op 的 `seq`、`base_seq`、幂等键、actor、冲突规则。

当前 `crates/store/migrations/0001_initial.sql` 已有：

- `workspaces`
- `versions`
- `proposals`
- `messages`
- `runs`
- `run_steps`
- `run_events`
- `uploads`
- `artifacts`
- `providers`
- `cost_ledger`

它缺少：

- `canvases`
- `canvas_ops`
- `canvas_presence`
- 可选的 `canvas_comments`

前端 `web/src/types.ts` 当前暴露的是 `WorkbenchState.graph.nodes[] / edges[]`，仍是执行图视图，不是完整 canvas document。

## 目标后端模型

### CanvasDocument

`CanvasDocument` 是后端保存的画布 snapshot。它保存完整画布状态，但不直接等于执行计划。

推荐字段：

```json
{
  "schema_version": 1,
  "canvas_id": "canvas_01",
  "workspace_id": "ws_01",
  "document_version_id": "ver_01",
  "title": "Product video workflow",
  "seq": 42,
  "base_graph_version_id": "ver_graph_01",
  "viewport": {
    "x": 0,
    "y": 0,
    "zoom": 1
  },
  "nodes": {},
  "edges": {},
  "comments": {},
  "metadata": {},
  "created_at": "2026-07-01T00:00:00Z",
  "updated_at": "2026-07-01T00:00:00Z"
}
```

字段规则：

- `schema_version`: 当前为 `1`。
- `canvas_id`: 画布 id。一个 workspace 可以先只绑定一个 canvas，后续再扩展多 canvas。
- `workspace_id`: 所属 workspace。
- `document_version_id`: 当前 canvas snapshot 的版本 id。可以复用现有 versions，也可以独立 canvas snapshot 版本。
- `seq`: 已应用的最后一个 durable op 序号。
- `base_graph_version_id`: 当前执行图投影对应的 graph version。用于兼容现有 run/proposal 逻辑。
- `viewport`: 仅保存全局默认视口；每个用户的实时视口应进入 presence。
- `nodes`: `CanvasNode` map，key 为 node id。
- `edges`: `CanvasEdge` map，key 为 edge id。
- `comments`: `CanvasComment` map，key 为 comment id。
- `metadata`: 扩展信息，只允许后端明确支持的 key。

### CanvasNode

节点是画布的最小持久实体。执行节点只是节点的一种。

```json
{
  "id": "node_video_01",
  "kind": "workflow",
  "node_type": "video.atlas.text_to_video",
  "title": "Video",
  "position": { "x": 220, "y": 0 },
  "size": { "width": 240, "height": 160 },
  "ports": {
    "inputs": [
      { "name": "prompt", "type": "text", "required": true }
    ],
    "outputs": [
      { "name": "video", "type": "video" }
    ]
  },
  "params": {
    "prompt": "clean product shot",
    "duration_sec": 5,
    "resolution": "720P"
  },
  "content": {},
  "media": [],
  "runtime": {
    "status": "queued",
    "run_id": null,
    "run_step_id": null,
    "artifact_ids": [],
    "error": null
  },
  "ui": {
    "z_index": 1,
    "collapsed": false,
    "locked": false,
    "color": null
  },
  "created_at": "2026-07-01T00:00:00Z",
  "updated_at": "2026-07-01T00:00:00Z"
}
```

`kind` 推荐枚举：

- `workflow`: 可投影到 `WorkflowGraph` 的可执行节点。
- `text`: 文本卡、prompt 草稿、说明文字。
- `image`: 图片素材或生成结果节点。
- `video`: 视频素材或生成结果节点。
- `audio`: 音频素材或生成结果节点。
- `comment`: 画布内评论节点。
- `group`: 分组框。
- `artifact`: 指向 `artifacts` 表中产物的节点。
- `config`: 模型、provider、尺寸、批量参数等配置节点。

字段规则：

- `node_type` 只对 `workflow` 和需要进入执行投影的 `config` 节点必填。
- `params` 只保存执行参数，必须能被 registry 校验。
- `content` 保存非执行内容，例如 text node 的正文、comment 的 markdown。
- `media` 保存 upload/artifact/storage 引用，不直接内嵌大文件。
- `runtime` 是 job/run 回灌的当前状态摘要，不替代 `runs` / `run_steps` / `run_events`。
- `ui` 保存长期 UI 属性；临时 select/hover/cursor 不在这里。

### CanvasEdge

```json
{
  "id": "edge_video_output",
  "from": {
    "node_id": "node_video_01",
    "port": "video"
  },
  "to": {
    "node_id": "node_output_01",
    "port": "artifact"
  },
  "kind": "artifact",
  "label": null,
  "metadata": {},
  "created_at": "2026-07-01T00:00:00Z",
  "updated_at": "2026-07-01T00:00:00Z"
}
```

`kind` 推荐枚举：

- `data`: 普通执行数据边。
- `artifact`: 产物传递边。
- `control`: 控制/依赖边。
- `reference`: 非执行引用边。
- `visual`: 纯视觉连线，不进入执行投影。

只有 `data`、`artifact`、`control` 进入执行投影。`reference` 和 `visual` 留在 canvas document。

### CanvasComment

```json
{
  "id": "comment_01",
  "anchor": {
    "node_id": "node_video_01",
    "edge_id": null,
    "position": null
  },
  "body": "这里需要增加参考图输入。",
  "resolved": false,
  "author_id": "user_01",
  "created_at": "2026-07-01T00:00:00Z",
  "updated_at": "2026-07-01T00:00:00Z"
}
```

评论应是 durable 数据。评论 hover、输入框草稿、当前聚焦评论不应进入 durable op。

## 增量操作格式

### CanvasOpEnvelope

所有长期画布变更都用 append-only op 记录。后端给每个 accepted op 分配单调递增 `seq`。

```json
{
  "op_id": "op_01",
  "canvas_id": "canvas_01",
  "seq": 43,
  "base_seq": 42,
  "actor": {
    "id": "user_01",
    "kind": "user"
  },
  "kind": "node.patch",
  "payload": {
    "node_id": "node_video_01",
    "patch": {
      "params": {
        "duration_sec": 4
      }
    },
    "prev": {
      "params": {
        "duration_sec": 5
      }
    }
  },
  "idempotency_key": "client-uuid-01",
  "created_at": "2026-07-01T00:00:00Z"
}
```

字段规则：

- `op_id`: 后端生成。
- `seq`: 后端生成，canvas 内单调递增。
- `base_seq`: 客户端提交时看到的最后序号。
- `actor`: 用户、agent、system、job worker。
- `kind`: 操作类型。
- `payload`: 按 `kind` 校验，不能是任意结构透传。
- `idempotency_key`: 客户端生成，用于重试去重。

### Durable op 类型

第一阶段必须支持：

- `node.add`
- `node.patch`
- `node.move`
- `node.resize`
- `node.delete`
- `edge.add`
- `edge.delete`
- `comment.add`
- `comment.patch`
- `comment.delete`
- `run.request`
- `artifact.attach`
- `proposal.apply`

后续可增加：

- `group.add`
- `group.patch`
- `group.delete`
- `canvas.rename`
- `canvas.metadata.patch`
- `node.reorder`

### Presence 类型

以下不进入 `canvas_ops`：

- cursor position
- current viewport
- selected node ids
- selected edge ids
- hover target
- active inspector tab
- draft text before submit

这些通过 `canvas_presence` 或 WebSocket volatile message 维护。

## 冲突规则

后端必须在接受 op 前校验：

- `canvas_id` 存在。
- `base_seq` 不大于当前 `seq`。
- `idempotency_key` 未被同一 actor 使用过；已使用则返回原 accepted op。
- `node.add` 的 id 不存在。
- `node.patch` 的 node id 存在。
- `node.delete` 会删除相关 edge 或拒绝带 edge 的删除，二选一必须明确。
- `edge.add` 的两端 node 和 port 存在。
- `edge.add` 对单输入 port 不允许重复连接。
- `artifact.attach` 引用的 artifact 必须存在并属于同一 workspace。
- `run.request` 必须能从当前 canvas 投影出合法 `WorkflowGraph`。

字段级冲突建议：

- `node.move` / `node.resize`: last-write-wins，可以接受旧 `base_seq`。
- `node.patch.params`: 要求 `prev`，避免 agent 和用户同时改参数时互相覆盖。
- `node.patch.content`: 对纯文本第一阶段用 whole-field replace；后续若要多人同编再引入 CRDT。
- `edge.add` / `edge.delete`: 服务端按当前图校验，不信任客户端。

## DB 落地建议

现有表继续保留。新增最小表：

```sql
CREATE TABLE canvases (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  title TEXT NOT NULL,
  seq INTEGER NOT NULL DEFAULT 0,
  snapshot_path TEXT NOT NULL,
  snapshot_hash TEXT NOT NULL,
  current_version_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
  FOREIGN KEY (current_version_id) REFERENCES versions(id) ON DELETE SET NULL
);

CREATE TABLE canvas_ops (
  id TEXT PRIMARY KEY,
  canvas_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  base_seq INTEGER NOT NULL,
  actor_json TEXT NOT NULL,
  kind TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (canvas_id) REFERENCES canvases(id) ON DELETE CASCADE,
  UNIQUE (canvas_id, seq),
  UNIQUE (canvas_id, idempotency_key)
);

CREATE TABLE canvas_presence (
  canvas_id TEXT NOT NULL,
  actor_id TEXT NOT NULL,
  cursor_json TEXT,
  selection_json TEXT,
  viewport_json TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (canvas_id, actor_id),
  FOREIGN KEY (canvas_id) REFERENCES canvases(id) ON DELETE CASCADE
);
```

说明：

- `snapshot_path` 延续当前 graph JSON 文件存储模式，降低迁移风险。
- `canvas_ops` 是恢复、审计、同步、断线续传的依据。
- `canvas_presence` 可以后续加 TTL 清理。
- 如果评论需要全文检索或独立列表查询，再增加 `canvas_comments`；第一阶段可以存在 `CanvasDocument.comments` 内。

## API / WebSocket 边界

第一阶段 REST：

- `GET /api/workspaces/{workspace_id}/canvas`
  - 返回 `CanvasDocument` snapshot 和当前 `seq`。
- `POST /api/canvases/{canvas_id}/ops`
  - 接收单个或批量 `CanvasOpEnvelope` draft，返回 accepted ops。
- `GET /api/canvases/{canvas_id}/events?after_seq=42`
  - 返回 `seq > 42` 的 op，用于断线补齐。

第二阶段 WebSocket：

- `WS /api/canvases/{canvas_id}/connect?ticket=...`

客户端消息：

- `op.batch`
- `presence.update`
- `ack`

服务端消息：

- `sync`
- `op`
- `presence`
- `run.event`
- `error`

## 生成任务回灌流程

推荐流程：

1. UI 或 agent 提交 `run.request` op。
2. 后端从当前 `CanvasDocument` 投影出 `WorkflowGraph`。
3. 复用现有 `GraphService::compile_plan`。
4. 复用现有 `RunService` 创建 `runs` / `run_steps` / `run_events`。
5. 生成结果写入现有 `artifacts`。
6. 后端提交 `artifact.attach` op，或提交 `node.patch` 更新节点 `runtime.artifact_ids`。
7. 前端收到 op 后把产物显示为 artifact / image / video 节点。

重要边界：

- canvas 层不直接调用模型 provider。
- run 层不直接修改前端局部状态；它通过 store 和 canvas op 回灌。
- artifact 是生成结果的持久来源，canvas node 只引用 artifact id。

## CanvasDocument 到 WorkflowGraph 的投影

`WorkflowGraph` 继续作为执行层输入。投影规则：

1. 遍历 `CanvasDocument.nodes`。
2. 只选择 `kind = "workflow"` 的节点。
3. 节点必须有 `node_type`。
4. 生成 `GraphNode`：
   - `node_type = CanvasNode.node_type`
   - `title = CanvasNode.title`
   - `params = CanvasNode.params`
   - `pos = [position.x, position.y]`
5. 遍历 `CanvasDocument.edges`。
6. 只选择 `kind = "data" | "artifact" | "control"` 的边。
7. 两端都必须存在于投影节点集合中。
8. 生成 `GraphEdge`：
   - `from = [from.node_id, from.port]`
   - `to = [to.node_id, to.port]`
   - `edge_type = kind`，其中 `data` 可以按 port type 转换为 `text/image/video/audio/json/mask`。
9. 调用现有 `GraphService::validate_graph`。

非执行节点处理：

- `text` 节点默认不进入执行图。
- 如果 text 节点被用作 prompt 输入，第一阶段应通过 `node.patch.params` 写入目标 workflow node 的 `params.prompt`，不要让 text node 直接进入执行图。
- `artifact` 节点默认不进入执行图；如果要作为输入，转换为目标 workflow node 的 param 或 artifact edge。

## 本地差距清单

| 能力 | 当前状态 | 目标状态 | 优先级 |
| --- | --- | --- | --- |
| 执行图 `WorkflowGraph` | 已有 | 保留为投影结果 | P0 |
| 图校验和编译执行计划 | 已有 | 由 canvas 投影后复用 | P0 |
| run / run_step / run_event | 已有 | 由 `run.request` 触发 | P0 |
| artifact 表 | 已有 | 由 `artifact.attach` 回灌引用 | P0 |
| `CanvasDocument` 类型 | 缺失 | 新增后端模型 | P0 |
| `CanvasOpEnvelope` 类型 | 缺失 | 新增 durable op 模型 | P0 |
| canvas -> graph 投影 | 缺失 | 新增纯函数和单测 | P0 |
| canvas op apply | 缺失 | 新增增量应用和冲突检查 | P1 |
| `canvases` / `canvas_ops` 表 | 缺失 | 新增 migration 和 store API | P1 |
| REST canvas API | 缺失 | 新增 snapshot/op/event API | P1 |
| WebSocket 同步 | 缺失 | 新增 ticket + WS | P2 |
| presence | 缺失 | 新增 volatile selection/cursor/viewport | P2 |
| 前端完整 canvas state | 部分 | 从 `graph` 视图迁移到 canvas document | P2 |
| job 产物回灌为 canvas 节点 | 缺失 | `artifact.attach` / `node.patch` | P1 |

## 实现顺序

### 阶段 1：类型和投影

目标：不改变现有 API，不影响运行路径，先固定格式。

任务：

- 新增 `CanvasDocument`、`CanvasNode`、`CanvasEdge`、`CanvasOpEnvelope`。
- 新增 `CanvasService::project_workflow_graph`。
- 单测覆盖：
  - workflow 节点能投影成当前 `WorkflowGraph`。
  - text/comment/visual 节点不会进入执行图。
  - artifact/data/control 边按规则投影。
  - 缺失 node_type、缺失 endpoint、未知 executable kind 必须报错。

### 阶段 2：op apply

目标：让后端可以从 snapshot + ops 重放出当前画布。

任务：

- 实现 `CanvasService::apply_op`。
- 实现 `CanvasDocument::replay_ops`。
- 支持 P0/P1 op 类型。
- 单测覆盖幂等、`base_seq`、参数 `prev` 冲突、edge endpoint 校验。

### 阶段 3：store

目标：持久化 canvas snapshot 和 op log。

任务：

- 新增 migration。
- 新增 `canvas_records.rs`。
- 支持 create/get/list/append ops。
- append op 和更新 canvas seq 必须在同一个事务。

### 阶段 4：API

目标：前端可以真实读写 canvas。

任务：

- `GET /api/workspaces/{workspace_id}/canvas`
- `POST /api/canvases/{canvas_id}/ops`
- `GET /api/canvases/{canvas_id}/events`
- 将当前 workbench graph view 改为从 canvas projection 生成。

### 阶段 5：run 回灌

目标：生成结果真正变成画布节点或节点产物引用。

任务：

- `run.request` op 创建 run。
- run event 映射为 `node.patch.runtime`。
- artifact 创建后提交 `artifact.attach`。

### 阶段 6：WebSocket / presence

目标：多人或多端实时同步。

任务：

- ticket 获取。
- WS connect。
- `op.batch` 广播。
- `presence.update` volatile 广播。
- 断线后用 `events?after_seq` 补齐。

## 不采纳的方案

### 不把 Canvas.best snapshot 直接照搬成后端 source of truth

原因：

- 实测它没有远端 op log。
- add/move 直接写 IndexedDB snapshot。
- select 不持久化。
- 这适合单机 local-first，不足以支持后端审计、断线续传、多人同步、agent 与用户并发编辑。

### 不把 Higgsfield worker 架构完整照搬

原因：

- Helixflow 已有 workspace/version/proposal/run/artifact 分层。
- 第一阶段更需要稳定的后端数据格式和投影，不需要先引入复杂远端 worker。
- WebSocket 可以作为阶段 6，不应阻塞阶段 1-5。

## 第一阶段完成标准

第一阶段算完成必须满足：

- 有独立后端类型表达完整 `CanvasDocument`。
- 有独立 op envelope 类型表达 durable 增量操作。
- 有 canvas 到 `WorkflowGraph` 的投影函数。
- 现有 `GraphService::validate_graph` 和 `compile_plan` 可复用。
- 有 Rust 单测证明非执行节点不会污染执行图。
- `cargo test -p helixflow-graph` 通过。

这能把“画布格式”从调研结论变成仓库内可编译、可测试的后端契约。

## 2026-07-01 落地状态

本轮已经完成的实现：

- `crates/graph/src/canvas.rs`
  - `CanvasDocument`
  - `CanvasNode`
  - `CanvasEdge`
  - `CanvasComment`
  - `CanvasOpEnvelope`
  - `CanvasOpKind`
  - `CanvasDocument::project_workflow_graph`
- `crates/graph/src/canvas_ops.rs`
  - `CanvasDocument::apply_op`
  - `CanvasDocument::replay_ops`
  - `node.add` / `node.patch` / `node.move` / `node.resize` / `node.delete`
  - `edge.add` / `edge.delete`
  - `comment.add` / `comment.patch` / `comment.delete`
  - `run.request`
  - `artifact.attach`
  - `proposal.apply`
- `crates/store/migrations/0002_canvas.sql`
  - `canvases`
  - `canvas_ops`
  - `canvas_presence`
- `crates/store/src/canvas_records.rs`
  - canvas snapshot record API
  - op append API，后端分配 `seq`
  - idempotency key 去重
  - `events?after_seq` 所需查询
  - presence upsert
- `crates/server/src/workbench_canvas.rs`
  - workspace 首次读取时从当前 `WorkflowGraph` bootstrap `CanvasDocument`
  - canvas op 写入 store 后应用到 snapshot
  - `run_request` op 触发现有 `RunService`
  - run confirmation 后将 artifacts 写回 `artifact_attach` op
- `crates/server/src/main.rs`
  - `GET /api/workspaces/{workspace_id}/canvas`
  - `POST /api/canvases/{canvas_id}/ops`
  - `GET /api/canvases/{canvas_id}/events`
  - `POST /api/canvases/{canvas_id}/presence`
  - `WS /api/canvases/{canvas_id}/ws`
- `web/src/types.ts` / `web/src/api.ts`
  - typed canvas snapshot/op/event/presence API boundary
  - canvas WebSocket client helper

本轮没有做的部分：

- 没有把现有 React 画布 UI 的内部状态完全迁移到 `CanvasDocument`；现在只是补齐前端调用边界，原有 UI 仍主要消费 `WorkbenchState.graph`。
- 没有实现多人文本 CRDT；`node.patch.content` 第一阶段仍是 whole-field replace。

当前已补齐的兼容层：

- `POST /api/canvases/{canvas_id}/ticket` 返回本地短期 ticket；默认 local dev 为 disabled，`HELIXFLOW_CANVAS_TICKET_MODE=required` 时 WebSocket 需要携带有效 ticket。
- `GET /api/workspaces/{workspace_id}/events?afterSeq=<seq>` 返回最新 run 的持久事件，用于 reconnect 和 seq gap catch-up。
- 前端 WebSocket helper 会在连接前 fetch missing events；收到跳号事件时先 REST catch-up，再应用新的 WS event。

完整功能的后续 spec / issue packet 已拆到：

- `specs/canvas-agent-full/product.md`
- `specs/canvas-agent-full/tech.md`
- `specs/canvas-agent-full/tasks.md`
- `specs/canvas-agent-full/issues.md`

本轮验证：

- `cargo test -p helixflow-graph`
- `cargo test -p helixflow-store`
- `cargo test -p helixflow-server`
- `cargo check --workspace`
- `cargo test --workspace`
- `npm run build`
- `npm test`
