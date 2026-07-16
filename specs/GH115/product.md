# Product Spec

## Linked Issue

GH-115：<https://github.com/majiayu000/helixflow/issues/115>

## 用户问题

Helixflow 在没有配置 runtime provider 时会自动选择 `mock`，并把 mock 产生的 PNG
文件头或普通文本 MP4 当成真实媒体 artifact。用户因此看到 step/run 为
`succeeded`，却没有得到可用的图片或视频；生产部署也无法区分“真实 provider 已接通”
和“本地测试替身在运行”。

## 目标

- 未显式配置可用 runtime provider 的生产启动必须 fail closed，workspace 不得隐式
  选择可执行的 `mock`。
- `mock` 只能通过明确的开发/测试配置启用，并在 provider catalog 中持续标识为
  non-production local test provider。
- 声明为 PNG 或 MP4 的 artifact 必须在 step/run 成功前通过内容校验；无效媒体必须让
  当前 step/run 失败并返回可读原因。
- 保持显式注入 mock 的 Rust 测试和本地开发能力，不要求真实 provider 或付费调用。

## 非目标

- 不修改 `web/**` 或重做 provider picker UI。
- 不改变 Atlas/FAL 的认证、模型参数、计费或远端任务取消协议。
- 不把 mock 输出描述为真实生成，也不以 warning、占位文件或降级成功代替错误。
- 不为所有图片/视频编码实现通用播放器或完整编解码器。

## Behavior Invariants

1. **P1 — 未配置时 fail closed。** 当 `HELIXFLOW_RUNTIME_PROVIDER` 缺失、为空或只含空白时，provider catalog 与新 workspace 的 `defaultProvider` / `selectedProvider` 必须指向一个明确的 unavailable 状态；任何需要 runtime provider 的 run 不得由 `mock` 代执行，也不得以 `succeeded` 结束。
2. **P2 — mock 必须双重显式启用。** 只有同时显式选择 `HELIXFLOW_RUNTIME_PROVIDER=mock` 且开启专用开发/测试 mock 开关时，`mock` 才能注册为 enabled 并可执行；只设置其中一项时必须 unavailable/fail closed。
3. **P3 — mock 身份不可伪装。** 启用后的 catalog 必须把 `mock` 标识为 `local_test` / non-production synthetic provider，不能使用会被理解为真实生成 provider 的状态文案。
4. **P4 — PNG 内容校验。** provider 声明输出为 `image/png` 时，内容必须具备完整、结构一致的 PNG 数据；只有签名字节、截断 chunk、缺失图像数据或结束 chunk 的内容必须在 artifact 记录和 step 成功前失败。
5. **P5 — MP4 内容校验。** provider 声明输出为 `video/mp4` 时，内容必须具备结构一致的 ISO BMFF/MP4 基本盒结构和媒体数据；普通文本、截断 box 或缺少必要盒的内容必须在 artifact 记录和 step 成功前失败。
6. **P6 — 失败状态可信。** 任何 P4/P5 校验错误必须使对应 step 为 `failed`、run 为 `failed`，不得留下数据库 artifact 记录、不得发出 `run.succeeded`，错误信息应说明媒体类型和校验失败，不泄露 URL、凭据或本地绝对路径。
7. **P7 — 有效显式测试路径可用。** 显式注入/启用的 mock 可以产生可通过同一 validator 的确定性测试媒体，使测试不依赖网络或真实 provider；相同请求保持确定性。
8. **P8 — 既有真实 provider 兼容。** Atlas/FAL 下载到的合法 PNG/MP4 继续落盘和成功；HTTPS、错误传播和 secret redaction 约束不放宽。

## 验收标准

- [x] 无 provider 环境变量时，registry/workspace 状态为 unavailable，运行 provider 节点明确失败，不出现隐式 mock success。
- [x] 仅 `HELIXFLOW_RUNTIME_PROVIDER=mock` 或仅 mock 开关时，mock 不可执行；两者同时设置时才 enabled。
- [x] catalog 明确包含 `local_test` 与 non-production/synthetic 文案。
- [x] 无效 PNG 和文本 MP4 的 focused tests 先稳定复现旧实现会接受，再证明修复后 step/run 失败且不创建 artifact。
- [x] 显式 mock 产生的 PNG/MP4 通过生产持久化 validator，相关 run 测试继续成功。
- [x] `cargo check --workspace`、`cargo test --workspace` 与 SpecRail checks 通过。

## 边界情况

- 空白、大小写错误或未知 provider id 不得退回 mock。
- mock 开关只接受明确的真值；未识别值按关闭处理，避免配置拼写错误导致生产启用。
- 媒体 payload 的 MIME 与 kind 不一致时按无效响应失败。
- 远端 HTTPS 下载成功但内容无效时仍须失败，不能仅凭 HTTP 200 或扩展名接受。
- 校验失败发生在数据库 artifact 创建前；文件写入也不应保留无效媒体。

## 发布说明

这是有意的默认行为变更：未配置 provider 的部署不再自动使用 mock。需要本地测试媒体的
开发者必须显式设置 `HELIXFLOW_RUNTIME_PROVIDER=mock` 和专用 mock 开关。生产部署应选择
并配置 Atlas/FAL 等真实 provider。现有 workspace 若持久化选择了未注册 provider，会继续
显示 unavailable，而不会静默回退。
