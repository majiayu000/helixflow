# Product Spec

## Linked Issue

GH-61

## 用户问题

GH-57 设计了可插拔 runtime provider,但如果只有 Atlas/mock 两类路径,很难证明 provider 抽象不是单家特化。用户需要 fal.ai 作为第二个真实 provider:同一张工作流图不改节点,只切换 workspace provider 选择,即可在 fal.ai 上完成至少文生图能力。

## 目标

- `fal` 作为 runtime provider 出现在 workspace provider 列表中。
- 配置 `FAL_KEY` 后,文生图工作流可通过 fal.ai 端到端产出图片 artifact。
- 未配置 `FAL_KEY` 时,`fal` 明确标记 unavailable,运行被拒绝,不降级到 mock 或 Atlas。
- provider 切换只改变 workspace provider 选择,不要求用户修改图上的节点类型或参数。

## 非目标

- 不同步 fal.ai 全量模型目录。
- 不实现 fal.ai 全能力矩阵,本 issue 只要求代表性的文生图能力。
- 不做 BYOK UI、密钥热加载、计费配额或跨 provider 自动择优。

## Behavior Invariants

1. workspace state 的 `providers.runtimeProviders` 至少能同时展示 `mock`、`atlas` 和 `fal`;一个 provider 不可用不影响其他 provider 的可选状态。
2. 未设置 `FAL_KEY` 时,`fal` 的状态为 unavailable,原因文案可读且不包含密钥、bearer token、本地路径或原始上游响应。
3. 用户选择 unavailable 的 `fal` 发起 run 时,请求明确失败并显示 provider 不可用原因,不产生 mock 占位产物。
4. 设置有效 `FAL_KEY` 并选择 `fal` 后,现有 `image.generate` 节点可执行并返回真实图片 artifact;输出预览和下载沿用现有 artifact content 端点。
5. 同一工作流在 `atlas` 与 `fal` 之间切换时,节点类型、连线和参数不需要修改;差异只来自 provider 选择。
6. fal.ai 上游认证失败、限流或任务失败时,run 以 failed 收敛并展示脱敏错误;不得记录或返回 `FAL_KEY`。
7. fal.ai 暂不支持的 capability 被请求时,run 明确失败为 unsupported/unavailable,不得静默用其他 provider 执行。

## 验收标准

- [ ] 设置 `FAL_KEY` 后,文生图工作流选择 `fal` 可端到端生成图片 artifact。
- [ ] 不设置 `FAL_KEY` 时,`fal` 在 provider 列表中 unavailable,运行被明确拒绝。
- [ ] 同一张图只切换 provider 选择即可分别用 Atlas/fal.ai 跑通文生图。
- [ ] 错误消息与 API 响应中不出现密钥或 bearer token。

## 边界情况

- `FAL_KEY` 在服务运行期间变化:本 issue 不要求热加载,重启后生效即可。
- fal.ai 返回异步队列任务:用户只看到 Helixflow run 进度和最终 artifact,不暴露上游 raw task id 作为可下载地址。
- fal.ai 返回 remote URL:服务端必须按 artifact 契约落盘或通过安全 content 端点提供,不把上游 URL 作为最终 `storage_uri` 暴露给前端。

## 发布说明

需要在部署环境设置 `FAL_KEY`;未设置时功能以 unavailable provider 形式显式呈现。本变更依赖 GH-57 provider 框架实现。
