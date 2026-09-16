# Helixflow 图片编辑能力 · Tech Spec

## 旧数据流

```text
Web -> Helixflow proxy -> media-processor:5578 -> Atlas -> media-processor output
    -> Web download -> Web upload -> Helixflow upload -> canvas result node
```

问题是同一动作横跨两个任务系统和两个本地进程；浏览器还承担 Base64 编码、轮询、下载和
再次上传。任何一段未启动都会让已有 UI 变成空壳。

## 新数据流

```text
Canvas editor
  -> multipart POST /api/workspaces/:id/image-processing-jobs
  -> Helixflow durable job (queued) + HTTP 202
  -> Helixflow background execution (running)
  -> selected workspace Provider submit/poll
  -> inpaint protected-pixel composition + output validation
  -> Helixflow workspace upload + succeeded
  -> Web polls job, appends result node
  -> PUT result-node linkage only
```

Helixflow server 是任务状态和文件落盘的唯一写入者。前端只提交意图与源图，并在成功后创建
结果节点。浏览器断开不会中断已提交任务，也不能把 Provider 任务写成成功或失败。

## HTTP 契约

### `GET /api/workspaces/:workspaceId/image-processing-capabilities`

返回当前工作区 Provider 的可用 profile：

```ts
type ImageProcessingCapabilities = {
  provider: string;
  defaults: Partial<Record<ImageCanvasToolKind, string>>;
  profiles: Partial<Record<ImageCanvasToolKind, Array<{
    name: string;
    model: string;
    usesSizeQuality: boolean;
  }>>;
};
```

Provider 未配置或不支持整套能力时返回明确错误，不由前端补默认值。

### `POST /api/workspaces/:workspaceId/image-processing-jobs`

`multipart/form-data`：

- `request`：JSON，包含 `sourceNodeId`、`intent`、可选 `profile` 和 typed `parameters`。
- `file`：源图。inpaint 的 file 已由蒙版编辑器写入透明 alpha。

后端验证输入、创建任务并启动后台执行，立即以 HTTP 202 返回：

```ts
{ job: ImageProcessingJob } // queued 或 running
```

Provider 或输出处理失败时，后台执行器将任务写为 failed。HTTP 断开不改变任务状态。

### `GET /api/workspaces/:workspaceId/image-processing-jobs/:jobId`

返回最新任务。前端在 `queued/running` 时有界轮询；`succeeded` 必须已有
`outputUploadId`，图片尺寸从 workspace upload 解码，不另外保存第二份输出元数据。

### `PUT /api/workspaces/:workspaceId/image-processing-jobs/:jobId`

前端只可提交 `{ resultNodeId, outputUploadId }`。任务必须已经是 succeeded，且 upload ID
必须等于该任务的输出。该请求只补充画布关联，不改变执行状态或完成时间。

## Gateway 边界

不新增通用媒体平台。现有 `ProviderRegistry` 增加一条图片快捷能力入口：

- 选择 workspace Provider。
- Atlas 实现 profile/model 映射、输入预处理、上传、submit/poll 和输出校验。
- 未配置 Provider 与不支持的 Provider fail closed。

Atlas 使用现有 `ATLAS_API_BASE` / `ATLAS_API_KEY` 配置，不增加第二套
`MEDIA_PROCESSOR_*` 配置。

## 输入与输出规则

| Intent | 后端准备 | Provider body | 校验 |
| --- | --- | --- | --- |
| outpaint | 按四边扩展透明画布 | edit model + prompt + 原生比例/尺寸 | 缩放完整 Provider 输出到编辑画布，不硬贴原图 |
| inpaint | 验证已有透明/非透明像素，并保留 alpha mask | edit model + prompt + 原生比例/尺寸 | 将原图非透明区域确定性回贴 |
| cutout | 统一编码 PNG | remove-background model | 同尺寸且有透明背景和前景 |
| upscale | 统一编码 PNG | provider-specific scale body | 宽高等于源图乘 scale |
| enhance | 统一编码 PNG | photo-cleanup body | 同尺寸 |

GPT Image 2 使用保持画布长宽比、边长为 16 倍数且符合所选 tier 的自定义 `size`。Nano
Banana 2 使用 `aspect_ratio`、`resolution` 和 `media_resolution`。GPT 自定义尺寸同时满足
至少 1024×1024 的像素预算。若供应商输出尺寸不同，先缩放到编辑画布尺寸；inpaint 再
回贴所有非透明像素。最终 outpaint/inpaint 输出尺寸等于准备后的编辑画布尺寸。

Outpaint 不做完整原图回贴。生产输出证明 Provider 会对整个编辑画布重建；强行回贴会在
原图边界产生结构接缝，feather 只能移动接缝。当前保证是源节点与源 upload 永不修改，
派生结果采用 Provider 的连续完整输出。只有 Provider 契约未来支持显式锁定 mask 时，才
能同时承诺派生结果内的像素级原图保护。

所有输入解码均限制字节数和画布尺寸；输出下载限制协议、字节数、图片解码和尺寸。模型与
profile 使用封闭的已支持列表，不接受客户端直接传 operation/model ID。

## 数据模型

`image_processing_jobs` 只保存 Helixflow 语义：

- `intent`
- `profile`
- `provider` / `model` / `provider_task_id`
- `output_upload_id`
- `source_node_id` / `result_node_id`
- `status` / `error` / timestamps

删除 `processor_kind` 和 `processor_job_id`，因为它们属于已移除的外部协议。

## 影响文件

- Gateway：图片 profile、Atlas 执行与 ProviderRegistry 路由。
- Server：图片能力/执行路由、upload 复用、删除固定代理。
- Store：移除 processor 字段，保存 provider task provenance。
- Web：同源图片能力 client、multipart 执行、删除 processor client/proxy。
- Tests/spec：新契约和五种映射覆盖。

## 验证

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test -p helixflow-gateway --locked
cargo test -p helixflow-store --locked
cd web
npm test -- --run src/image-canvas-tools.test.ts src/api-image-processing.test.ts
npm run build
cd ..
git diff --check
```

真实付费生成只在明确允许时运行；自动验证使用请求构造、mock HTTP、像素级合成断言与
持久化测试。
