# Product Spec

## Linked Issue

GH-57

## 用户问题

origin/main 上唯一可运行的执行 provider 是 MockProvider:未设置 `HELIXFLOW_RUNTIME_PROVIDER=mock` 时,服务器回退到 UnavailableProvider 占位(crates/server/src/app_state.rs:195-210)。产品无法产出真实图像/视频,用户也无法在界面上选择执行后端——provider 是编译期/环境变量写死的单选。

用户需要的是**可插拔且用户可选**的 runtime provider:多个 provider(Atlas、fal.ai、mock 等)可以同时注册,用户在前端选择用哪个执行工作流,选择持久化;没有配置凭据的 provider 在 catalog 中被诚实标记为不可用,运行被明确拒绝,而不是静默降级到 mock。

## 目标

- gateway 定义统一的 Provider trait(已存在)与多 provider 注册机制,多个 provider 可同时注册并出现在 catalog 中。
- 用户可在前端按 workspace 选择执行 provider,选择持久化,重启后仍然生效。
- Atlas 作为第一个真实 provider 接入:设置 `ATLAS_API_KEY` 后,文生图工作流端到端产出真实图片产物。
- provider 凭据只从环境变量/本地配置读取,绝不入库、不入日志、不进 agent ctx、不进 API 响应。
- 无 key 的 provider 在 catalog 中标记不可用;对其发起运行被明确拒绝,禁止静默降级到 mock(对齐 vibeguard U-29)。

## 非目标

- fal.ai 适配(独立 issue GH-61)。
- ComfyUI provider。
- 计费/配额管理。
- 节点级 provider 路由实现(仅在 tech spec 备选方案中记录,不实现)。
- BYOK UI / 在前端输入或管理凭据。

## Behavior Invariants

用编号列表写可观察、可测试、无实现细节的行为契约。

1. workspace state 的 `providers.runtimeProviders` 列出所有已注册 provider(至少 `mock` 与 `atlas`),每个条目带各自独立的 `enabled`/`status`/`message`/`capabilities`;一个 provider 不可用不影响其他 provider 的状态展示。`GET /api/registry/catalog` 保持纯节点 catalog,不承载 provider 状态;前端 provider 选择 UI 的数据源是 workspace state。
2. 未设置 `ATLAS_API_KEY` 时,`atlas` 条目为 `enabled=false`、`status="unavailable"`,并带可读的不可用原因;该原因不含任何凭据、URL 或本地路径。
3. 用户为 workspace 选择了不可用的 provider 后发起运行,请求被明确拒绝并返回可读错误;不产生任何 mock 或占位产物,run 不进入执行状态。
4. 为 workspace 选择 provider 后,该选择持久化:服务重启、页面刷新后 workspace state 仍返回同一选择;未做过选择的 workspace 使用服务器默认 provider。
5. 选择一个未注册的 provider id 时请求被拒绝(4xx),workspace 原有选择不变。
6. 设置了有效 `ATLAS_API_KEY` 且 workspace 选择 `atlas` 时,文生图工作流端到端完成,产出真实图片 artifact(非确定性 mock 产物);产物字节由服务端持久化,页面刷新与服务重启后仍可预览/下载。
7. `ATLAS_API_KEY` 无效(上游返回 401/403)时,run 以明确的失败状态结束,错误消息说明 provider 鉴权失败;消息与日志中不出现 key、bearer token 或原始响应中的敏感头。
8. 运行进行中切换 workspace 的 provider 选择,不影响 in-flight run(该 run 继续用发起时的 provider);切换仅对后续 run 生效。
9. mock 与真实 provider 并存:mock 始终注册、可被选择、可正常运行,provider label 明确标识其为本地测试 provider;画布上的可执行节点(文生图/文生视频)是 provider 中立的,节点名称与 label 不含任何 provider 名(包括 Mock),同一个图无需修改即可在 mock 与 atlas 下运行;选择 atlas 的 workspace 与选择 mock 的 workspace 互不影响。
10. 凭据(`ATLAS_API_KEY` 等)在任何 API 响应、数据库行、服务日志、agent ctx 文件中都不出现;catalog/错误消息经过与 GH-55 一致的脱敏处理(含大小写变体的 bearer 标记)。
11. workspace 持久化的 provider id 在当前 build 未注册时(如降级部署),workspace state 的 `providers.runtimeProviders` 中出现该 id 的合成 unavailable 条目(`enabled=false`,带可读且脱敏安全的原因文案),`selectedProvider` 保持用户的持久化选择不被改写;对其发起运行被明确拒绝。

## 验收标准

- [ ] 多 provider 注册生效:catalog 同时包含 mock 与 atlas,各自独立健康状态。
- [ ] 用户在前端选择 workspace 的执行 provider,选择持久化并在重启后保留。
- [ ] 设置 `ATLAS_API_KEY` 后,文生图工作流端到端产出真实图片产物。
- [ ] 无 key 时 atlas 在 catalog 中标记不可用,运行被明确拒绝,不静默降级到 mock。
- [ ] 密钥脱敏测试覆盖(沿用 GH-55 的 redaction 逻辑,含 lowercase bearer 用例)。

## 边界情况

- `ATLAS_API_KEY` 在服务运行期间变化:凭据只在启动时读取,变化需重启生效;spec 不要求热加载。
- workspace 持久化的 provider id 在当前 build 不再注册(如降级部署):workspace state 在 `runtimeProviders` 中为该 id 合成一个 unavailable 条目(前端因此能渲染其状态与原因,而不是拿到一个无匹配条目的 `selectedProvider`),运行被拒绝,不自动改写用户的选择;合成条目的原因文案同样不得包含会触发脱敏兜底的子串(如 `api_key`、`token`)。
- 选择请求体为空字符串或非法 id:拒绝并保持原选择。
- 并发对同一 workspace 发出选择请求:最后写入者生效,不产生部分写入状态。
- 上游 Atlas 请求超时或网络错误:与 401 同样以明确失败结束 run,消息脱敏。
- 不可用原因文案本身不得包含会触发脱敏兜底的子串(如 `api_key`、`token`),否则会被替换为通用文案,用户看不到真实原因。

## 发布说明

- 需要一次 SQLite migration(workspaces 表新增可空列),向前直接生效;老数据行选择为空,行为等同选择服务器默认 provider,无需数据回填。
- `HELIXFLOW_RUNTIME_PROVIDER` 环境变量语义变化:从"唯一 provider 开关"变为"服务器默认 provider id";未设置时默认仍为 `mock`,本地开发体验不变。
- 可执行节点类型更名为 provider 中立命名(`image.generate`/`video.text_to_video`),旧的 `image.mock.generate`/`video.mock.text_to_video` 删除且无兼容层;存量数据库中引用旧节点类型的图在新版本下无法通过校验,需重建(产品未发布,owner 接受该破坏性变更)。
- 启用 Atlas 需要在服务器环境设置 `ATLAS_API_KEY`(可选 `ATLAS_API_BASE`);文档需说明凭据只读环境变量、绝不入库。
