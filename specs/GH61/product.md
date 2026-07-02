# Product Spec

## Linked Issue

GH-61

## 用户问题

当前 provider 框架只有单个真实外部实现,无法证明运行时 provider 选择是可插拔能力而不是单家后端特化。用户需要在同一工作流上只切换 provider,就能用 fal.ai 作为第二个真实后端执行文生图。

## 目标

- fal.ai 作为第二个 runtime provider 出现在 provider catalog / workspace state 中。
- 配置 `FAL_KEY` 后,用户能选择 fal.ai 并执行至少一个文生图能力。
- 同一图不改节点、不改连线,只改 workspace provider selection,即可在 Atlas 与 fal.ai 之间切换执行。
- 无 `FAL_KEY` 或 fal.ai 返回错误时,UI 和 API 给出明确不可用/失败原因。

## 非目标

- 不同步 fal.ai 全量模型目录。
- 不实现跨 provider 参数自动迁移或模型质量对齐。
- 不绕过 GH-57 的 provider framework、artifact persistence、provider selection 约定。

## Behavior Invariants

1. 选择 fal.ai 且 `FAL_KEY` 有效时,文生图工作流端到端生成 image artifact,artifact 走既有 preview/download 体验。
2. 同一工作流在 Atlas 与 fal.ai 间切换时,图结构和节点定义不需要改动;只有 workspace provider selection 变化。
3. 无 `FAL_KEY` 时,fal.ai 在 catalog/state 中显示为 unavailable,开始 run 前或估算阶段给出明确拒绝,不得静默 fallback 到 mock/Atlas。
4. fal.ai 不支持的能力必须被 provider capability gate 拒绝,错误包含 provider id 与 capability。
5. fal.ai 外部请求失败、超时、返回 malformed response 时,run 进入失败状态并保留可诊断错误,不得产生假的成功 artifact。
6. fal.ai secret 不出现在 logs、workspace state、artifact metadata、PR/error 文本中。

## 验收标准

- [ ] 配置 `FAL_KEY` 后,文生图工作流选择 fal.ai 能生成可预览 image artifact。
- [ ] 删除 `FAL_KEY` 后,fal.ai 显示 unavailable,run 被明确拒绝且无 provider fallback。
- [ ] 同一图在 Atlas/fal.ai 间切换执行时,节点和连线不变。
- [ ] fal.ai 失败响应进入 run failed,错误可诊断且不泄露 secret。

## 边界情况

- `FAL_KEY` 为空字符串、空白、错误 key。
- fal.ai queue job 长时间 pending、失败、返回非 image URL。
- 用户在 run in-flight 期间修改 provider selection;当前 run 继续使用创建时解析的 provider。

## 发布说明

需要在部署文档中增加 `FAL_KEY` 配置说明。未配置时该 provider 只作为 unavailable 选项展示。

