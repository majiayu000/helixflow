# Tech Spec

## Linked Issue

GH-57

## Product Spec

见 `specs/GH57/product.md`。

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Provider trait 与 mock | `crates/gateway/src/lib.rs` | 定义 `Provider` trait(id/health/catalog/estimate/invoke/cancel)、`MockProvider`、catalog 类型(`ProviderCatalogSnapshot` 等)、`ProviderError` | trait 与注册机制的归属地;Atlas 实现要复用这些类型 |
| Runtime provider 封装 | `crates/gateway/src/runtime_provider.rs` | `RuntimeProvider` enum 只有 `Mock`/`Unavailable` 两个 variant;`catalog_snapshot()` 只产出单 provider 视图;`is_safe_provider_message`/`sanitize_provider_id`(约 205 行附近)是 GH-55/GH-56 建立的脱敏先例(含 lowercase bearer) | 多 provider 注册要扩展此处;所有对外文案必须过同一脱敏层 |
| Provider 选择入口 | `crates/server/src/app_state.rs:194-214` | `default_runtime_provider()` 只认 `HELIXFLOW_RUNTIME_PROVIDER=mock`,其他值一律 `UnavailableProvider`;`AppState.runner: RunService<RuntimeProvider>` 持有单一 provider | 改为构造 registry 并保留该 env 作为"服务器默认 provider id" |
| Node catalog API | `crates/server/src/main.rs:65`、`crates/server/src/registry_routes.rs` | `GET /api/registry/catalog` 只返回 `NodeRegistry::builtin().export_catalog()`(纯节点 catalog,无 provider 字段);`web/src/api.ts::fetchNodeCatalog` 按 `NodeCatalog` 解析 | 该端点保持不动;provider 状态另走 workspace state(见设计方案 §5 的 API 边界) |
| 节点定义 | `crates/registry/src/lib.rs:366-407` | 可执行图像/视频节点是 `image.mock.generate`(label "Mock Image")与 `video.mock.text_to_video`(label "Mock Text To Video"),定义里 `provider: Some("mock")` 绑定死 provider | 需改为 provider 中立节点定义,capability 保留、provider 绑定移除 |
| Plan 编译 | `crates/graph/src/lib.rs:266-301` | `GraphService::compile_plan(graph, version_id)` 从节点定义取 `definition.provider` 写入 `ExecutionStep.provider` | provider 的唯一来源要从"节点定义"换成"编译参数"(workspace 选择) |
| Workspace state | `crates/server/src/workspace_state.rs` | state payload 含 `providers`(GH-40 契约,含 `defaultProvider`/`runtimeProviders`/`apiConnectors`);`node_provider()` 从 node registry 默认取 `mock` | 增加 `selectedProvider`;节点投影的 provider 回退来源改为已解析的生效 provider |
| Run 执行路径 | `crates/run/src/lib.rs:163-420`、`crates/run/src/cost_gate.rs:71` | `RunService` 泛型持单一 provider;`execute_manual_run`(lib.rs:208-211)在内部调 `compile_plan`;`request_agent_run` 在 cost_gate;invoke 用 `step.provider` 构造 `ProviderRequest`(lib.rs:408-411) | run 请求需携带已解析的 provider id 并透传给 `compile_plan`;registry 实现 `Provider` 后 `RunService<ProviderRegistry>` 按 `req.provider` 分发 |
| 产物持久化 | `crates/run/src/lib.rs:575`(`persist_artifact`)、`crates/server/src/artifact_routes.rs` | `persist_artifact` 只把 `ArtifactPayload.storage_uri` 字符串写库;preview/download 路由只回元数据 + `safe_download_uri`,没有任何组件落盘或回源真实字节 | Atlas 返回的真实图片字节需要存储契约与回源端点(见设计方案 §8) |
| 持久化 | `crates/store/src/lib.rs`、`crates/store/migrations/0001_initial.sql`、`crates/store/src/workspace_records.rs` | `workspaces` 表(id/name/cur_version_id/时间戳);`artifacts` 表已有 `storage_uri`/`mime` 列;`providers` 表已有 `upsert_provider_status` | workspace 级选择的持久化位置;artifacts 表复用现有列存相对路径 |
| 前端 gate 与展示 | `web/src/app.tsx:98-103`、`web/src/components/top-bar.tsx`、`web/src/types.ts`、`web/src/api.ts` | gate 只认 `defaultProvider` 对应条目 healthy 才允许 queue;TopBar 只读展示 provider 状态 | gate 改为看 workspace 已选 provider;TopBar 增加选择器;数据源是 workspace state 而非 catalog 端点 |
| 旧 Atlas 实现 | `git fetch origin salvage/local-workbench-atlas-20260702 && git show FETCH_HEAD:crates/server/src/provider.rs`(commit `4f3e5ea`,已推送到 origin,任意 checkout 可复现) | 完整 `AtlasProvider`:chat_completion / image generate/edit / text_to_video + 轮询,`ATLAS_API_KEY`/`ATLAS_API_BASE` 读 env,bearer 鉴权 | 捞回适配为新 registry 下的 gateway 模块 |

## 设计方案

### 1. Provider trait 与实现的归属:`helixflow-gateway`

`Provider` trait 已在 gateway,保持不动。新增:

- `crates/gateway/src/registry.rs` — `ProviderRegistry`(注册机制)。
- `crates/gateway/src/atlas.rs` — `AtlasProvider`,从 salvage 分支捞回适配(gateway 新增 `reqwest` 依赖)。

server 只做 env 读取与装配,不再持有 provider 实现(删除旧的单 provider 假设,不留兼容层)。

### 2. 注册机制:`ProviderRegistry` 自身实现 `Provider`

```rust
pub struct ProviderRegistry {
    providers: BTreeMap<String, RuntimeProvider>, // id -> provider
    default_provider: String,
}
```

- `RuntimeProvider` enum 扩展 `Atlas(AtlasProvider)` variant(保持 Clone/Debug,不引入 `Arc<dyn Provider>`)。
- `ProviderRegistry` 实现 `Provider`:`invoke`/`estimate`/`cancel` 按 `req.provider` 查表分发;查不到返回 `ProviderError::Unavailable`(明确拒绝,无 fallback)。
- `AppState.runner` 从 `RunService<RuntimeProvider>` 改为 `RunService<ProviderRegistry>`。`RunService` 的执行/分发逻辑不变,仅按 §6 增加 provider id 的透传(请求字段 → `compile_plan` 参数)。
- 注册装配在 `app_state.rs`:
  - `mock` 恒注册(`RuntimeProvider::mock()`),catalog label 明确标识为本地测试 provider(如 `Mock (local test)`)。
  - `atlas`:`AtlasProvider::from_env()` 有 `ATLAS_API_KEY` 则注册可用实例;无 key 则注册 `RuntimeProvider::unavailable("atlas", <安全原因文案>)`——catalog 里始终能看到 atlas 条目及其不可用原因。
  - `HELIXFLOW_RUNTIME_PROVIDER` 语义改为"默认 provider id",未设置默认 `mock`;指向未注册 id 时默认项为 unavailable 条目(维持现有 UnavailableProvider 语义)。

### 3. 选择粒度:workspace 级

用户在前端为每个 workspace 选择执行 provider。节点级路由列入备选方案,本 issue 不实现;`ProviderRequest.provider` 字段已具备节点级扩展空间。

### 4. 选择持久化:`workspaces` 表新增列

- migration `crates/store/migrations/0002_workspace_runtime_provider.sql`:
  `ALTER TABLE workspaces ADD COLUMN runtime_provider_id TEXT NULL;`
- `NULL` 表示未选择,运行时回退到 registry 的 `default_provider`。
- store 新增 `set_workspace_runtime_provider(workspace_id, provider_id)` 与查询扩展(`WorkspaceRecord` 增加 `runtime_provider_id: Option<String>`)。
- 新 API:`PUT /api/workspaces/{workspace_id}/provider`,body `{"providerId": "atlas"}`。
  - `providerId` 必须是 registry 中已注册的 id(可以是 unavailable 的,允许"先选后配 key");未注册 id 返回 400,原选择不变。
  - 响应返回更新后的 workspace provider 视图。

### 5. API 边界:catalog 端点保持纯节点,provider 状态走 workspace state

明确的端点边界(回应 GH-40 契约,避免混用):

- `GET /api/registry/catalog` **保持纯节点 catalog 不动**:继续只返回 `NodeRegistry::export_catalog()`,不增加任何 provider 字段;`web/src/api.ts::fetchNodeCatalog` 及其 `NodeCatalog` 解析不受影响。
- provider 目录与健康状态**继续经 workspace state 的 `providers` snapshot 暴露**(GH-40 现状):`runtime_providers` 由 registry 聚合生成,列出全部注册 provider,每项独立 `enabled`/`status`/`message`/`capabilities`;`default_provider` = 服务器默认 id。前端选择 UI 的数据源就是 workspace state,不新增 provider 目录端点。
- atlas 可用时 `kind: "external_api"`、`status: "healthy"`;无 key 时 `kind: "unavailable"`、`enabled: false`。
- `persist_runtime_provider_status` 改为对 registry 内每个 provider 逐一 upsert 到 `providers` 表。
- workspace state 的 `providers` payload 增加一个 key:`selectedProvider`(string,已解析的生效 provider id;未选择时等于 `defaultProvider`)。
- **持久化选择缺失于 registry 时的投影**:组装 workspace state 时,若 `workspaces.runtime_provider_id` 指向未注册的 id(如降级部署),在 `runtimeProviders` 中**合成一个 unavailable 条目**(id = 持久化的 id,`enabled=false`、`status="unavailable"`,原因文案形如 `provider is not registered in this build`——避开 `api_key`/`token`/`bearer` 等会被脱敏兜底替换的子串);`selectedProvider` 保持用户的持久化选择不改写,前端因此始终能渲染该选择的不可用状态与原因。

### 6. Provider 中立节点与编译期 provider 注入

现有 `image.mock.generate` / `video.mock.text_to_video` 节点把 provider 写死在节点定义里,且 label 带 "Mock"——选择 atlas 的用户仍要摆 Mock 节点才能出真实产物。改为:

- **节点定义声明"能力"而非绑定 provider**:`crates/registry/src/lib.rs` 的可执行节点改为 provider 中立的 `image.generate`(label "Generate Image")与 `video.text_to_video`(label "Text To Video"),描述文案不含 Mock;定义保留 `capability`(`image_generate`/`text_to_video`),`provider` 绑定移除(置 `None`)。`llm.prompt_writer` 同样移除 provider 绑定。catalog 的 `estimated_cost` key 从 `{provider}.{capability}` 改为按 capability 生成(计费/配额是非目标,估价目录不按 provider 分叉)。
- **`GraphService::compile_plan` 增加 provider selection 参数**:签名变为 `compile_plan(graph, version_id, provider_id: &str)`,编译时把该 id 写进每个 `ExecutionStep.provider`(节点定义不再提供该值)。
- **RunService 入口透传 workspace 选择**:`ManualRunRequest` 与 `AgentRunRequest` 增加必填字段 `provider: String`(已解析的生效 provider id);`execute_manual_run` / `request_agent_run` 原样传给 `compile_plan`,自身不做解析。解析(workspace 选择 → NULL 时服务器默认)由 server 入口在入队时完成(见 §7)。
- **mock 降级为一个普通 provider**:`image.mock.generate`/`video.mock.text_to_video` classType **删除,不留兼容层**;mock 作为 registry 中的普通 provider 实现同样的 `image_generate`/`text_to_video`/`prompt_writer` capability。所有现有引用这些 classType 的图与测试同步更新为新 classType(内置模板图、`crates/graph`/`crates/run`/`crates/agent`/`crates/server` 测试、`web/src/app.test.tsx`),列入任务(SP57-T9 及各 lane 所属文件)。
- `workspace_state.rs` 的 `node_provider()` 不再能从节点定义取到 provider:未执行过的节点投影 provider 改为显示已解析的生效 provider id(`selectedProvider`);已执行节点仍显示 run step 上固定的 provider。
- capability id(`image_generate` 等)是跨 provider 共享词汇,mock 与 atlas 同名 capability 语义一致。

### 7. 运行路径:queue 时解析并固定 provider

- `queue_workspace_run` 与 agent run 入口在入队时解析生效 provider id(workspace 选择 → NULL 时服务器默认),填入 run 请求的 `provider` 字段;`compile_plan` 将其写进每个 step,持久化到 run steps 的 `provider` 列;run 执行期间不再重新解析——运行中切换选择不影响 in-flight run。
- 入队前检查生效 provider 在 registry 中的健康状态:unavailable 则返回 409/422 明确错误,run 不入队,不产生任何产物(禁止降级,对齐 U-29)。

### 8. 产物存储契约:字节落盘 + 内容端点

现有 `persist_artifact` 只写 `storage_uri` 字符串,没有组件负责真实字节。新增契约:

- **落盘**:`RunService` 把 provider 返回的字节(`ProviderResponse` 内联字节;若 provider 返回 HTTPS URL 则先下载其内容)写入 `data_dir` 下 `artifacts/{run_id}/{step_index}.{ext}`(ext 由 mime 推导);`RunService` 构造时由 server 注入 artifacts 根目录(`data_dir/artifacts`)。mock 产物迁移到同一落盘契约,消除双轨。
- **artifacts 表**:`storage_uri` 存相对 `data_dir` 的路径(即 `artifacts/{run_id}/{step_index}.{ext}`);绝不存上游 URL(signed URL 可能含凭据),`mime` 列照实填写。
- **回源端点**:新增 `GET /api/artifacts/{artifact_id}/content`——按 artifacts 行的 `mime` 设置 Content-Type 回源文件字节;路径必须规范化后校验落在 artifacts 根目录内(防路径穿越,对齐 SEC-07);文件缺失返回 404,不回占位内容。
- **preview/download 走它**:`safe_download_uri` 与前端预览/下载改为指向该 content 端点;现有"不暴露原始 storage_uri"的测试语义保留。

### 9. Atlas provider(第一个真实 provider)

- 从 salvage 捞回:`git fetch origin salvage/local-workbench-atlas-20260702 && git show FETCH_HEAD:crates/server/src/provider.rs`(commit `4f3e5ea`,分支已推送到 origin)。`from_env()` 读 `ATLAS_API_KEY`/`ATLAS_API_BASE`(默认 `https://api.atlascloud.ai/v1`)、bearer 鉴权、`image_generate` 同步生成、`text_to_video` 提交 + 轮询、`chat_completion`。本 issue 验收只卡 `image_generate` 端到端;其余 capability 捞回后以单测覆盖,不作为验收阻塞。
- `ProviderError` 新增上游失败 variant(如 `RequestFailed { provider, status, message }`);message 在进入任何日志/事件/API 响应前必须经 `is_safe_provider_message` 同源的脱敏处理。
- 注意:unavailable 原因文案不得包含 `api_key`/`token` 等子串(`is_safe_provider_message` 会把它兜底替换成通用文案),文案写成如 `Atlas credentials are not configured; set the Atlas environment before enabling this provider` 的形式,并加测试锁定。

### 10. 前端选择 UI:TopBar

- `web/src/components/top-bar.tsx` 增加 provider 下拉选择器:列出 `providers.runtimeProviders`(数据源是 workspace state,见 §5),每项显示 label + 状态徽标(healthy/unavailable);当前值为 `providers.selectedProvider`。持久化选择未注册时,§5 的合成条目保证下拉框仍能渲染该项的 unavailable 状态。
- 选择触发 `PUT /api/workspaces/{id}/provider`(`web/src/api.ts` 新增函数),成功后刷新 workspace state。
- `web/src/app.tsx` 的 `providerReady` gate 从"`defaultProvider` 对应条目 healthy"改为"`selectedProvider` 对应条目 enabled 且 healthy"。
- 不做设置面板、不做凭据输入。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 多 provider 状态经 workspace state 暴露,catalog 端点保持纯节点 | `registry.rs` 聚合 snapshot、`workspace_state.rs`;`registry_routes.rs` 不改 | `cargo test -p helixflow-gateway`;`cargo test -p helixflow-server workspace_state`;registry_routes 响应 shape 回归断言无 provider 字段 |
| P2 无 key 标记不可用 | `atlas.rs::from_env`、app_state 装配 | gateway 单测:无 env 时 snapshot 断言 `enabled=false` 且 message 安全 |
| P3 不可用 provider 拒绝运行 | `run_routes.rs` 入队检查 | `cargo test -p helixflow-server run_routes`(扩展 unavailable 测试) |
| P4 选择持久化 | migration 0002、store、`PUT .../provider` | store 单测 + server 路由测试:写入后重开 Store 仍读回 |
| P5 未注册 id 被拒 | provider 选择路由校验 | server 路由测试断言 400 与选择不变 |
| P6 Atlas 文生图 e2e,产物字节可预览/下载 | `atlas.rs::invoke(image_generate)`、`RunService` 落盘、`GET /api/artifacts/{id}/content` | run 单测:字节落盘 + `storage_uri` 相对路径;server 测试:content 端点 mime/404/路径穿越拒绝;手动验证:设 `ATLAS_API_KEY` 跑文生图 workflow,产物可预览/下载 |
| P7 上游 401 明确失败且脱敏 | `atlas.rs` 错误映射、`ProviderError::RequestFailed` | gateway 单测(mock http/固定 401 响应体)断言消息无 bearer/key |
| P8 运行中切换不影响 in-flight run | 入队解析 → run 请求 `provider` 字段 → `compile_plan` 参数固定到 run steps | graph 单测:`compile_plan` 把参数 id 写进每个 step;server run 测试:入队后改选择,断言 run 仍用原 provider |
| P9 mock 与真实并存,节点 provider 中立 | registry 装配、`crates/registry` 节点定义、前端 gate | gateway snapshot 测试含双条目;registry 单测断言节点无 provider 绑定且 label 不含 Mock;web `app.test.tsx` gate 用例 |
| P10 凭据零泄漏 | 脱敏层复用(`is_safe_provider_message`)、artifacts 不存上游 URL | 现有 redaction 测试保留 + 新增 atlas 消息脱敏用例 + run 落盘测试断言 `storage_uri` 非 http(s) |
| P11 持久化选择未注册时合成 unavailable 投影 | `workspace_state.rs` 组装 | server 测试:DB 写入未注册 id,state 断言合成条目 `enabled=false`、原因文案通过 `is_safe_provider_message`、`selectedProvider` 不被改写 |

## 数据流

- 输入:环境变量 `ATLAS_API_KEY`、`ATLAS_API_BASE`(可选)、`HELIXFLOW_RUNTIME_PROVIDER`(默认 provider id);前端 `PUT /api/workspaces/{id}/provider` 的 `providerId`。
- 持久化:`workspaces.runtime_provider_id`(仅 provider id 字符串,绝不存凭据);`providers` 表存各 provider 健康状态快照;产物字节落盘 `data_dir/artifacts/{run_id}/{step_index}.{ext}`,`artifacts.storage_uri` 存该相对路径。
- 运行链路:入队时解析生效 provider(workspace 选择 → 服务器默认)→ run 请求 `provider` 字段 → `compile_plan(graph, version_id, provider_id)` 写进每个 `ExecutionStep.provider` → 持久化 run steps → `ProviderRequest.provider` 分发到 registry。
- 输出:`GET /api/registry/catalog` 保持纯节点 catalog;workspace state `providers`(含 `selectedProvider` 与必要时的合成 unavailable 条目)承载 provider 目录;`GET /api/artifacts/{artifact_id}/content` 按 mime 回源产物字节,preview/download 指向它;run events 与现有链路一致。
- 外部调用:AtlasProvider 经 HTTPS 调 Atlas API(bearer 鉴权,key 仅存于进程内存中的 provider 配置结构);上游返回 URL 时由 RunService 下载后落盘,URL 本身不入库。
- agent ctx:继续走 GH-40 的安全 catalog 投影,新增 atlas 条目时同样不含任何 env 值。

## 备选方案

- 节点级 provider 路由:`ProviderRequest.provider` 已按节点携带,未来可让单个节点覆盖 workspace 选择;本 issue 只做 workspace 级,避免 UI 与 run 语义复杂化(owner 已拍板)。
- 保留 `image.mock.generate` 并新增别名/映射层:违反"无兼容层、无 alias"约束(U-24),且 Mock label 会继续误导 atlas 用户,拒绝。
- 新增独立的 provider 目录端点(如 `GET /api/providers`):workspace state 已承载同一数据(GH-40 契约),再加端点是重复来源;拒绝,前端数据源统一为 workspace state。
- 新建 `workspace_settings` 表存选择:更通用但引入 join 与第二张表的生命周期管理;当前只有一个设置项,直接加列到 `workspaces` 更简单,拒绝该方案。
- `Arc<dyn Provider>` 动态注册:更开放但丢失 Clone/Debug 且现阶段 provider 数量个位数;enum + registry 查表已满足"多注册、可插拔",拒绝过度设计。
- 产物字节存 SQLite blob:随图像/视频体积膨胀数据库,备份与流式回源都更差;文件落盘 + 相对路径入库更贴合现有 `data_dir` 布局,拒绝 blob 方案。
- 凭据热加载(运行期重读 env):增加状态同步复杂度,启动时读取 + 重启生效已满足产品决策,列为未来可选。

## 风险

- Security: 凭据泄漏是最大风险。缓解:key 只进 provider 内存配置;所有对外文案过 `is_safe_provider_message` 同源脱敏;artifacts 不存上游 URL;content 端点做根目录约束防路径穿越;新增测试断言 catalog/错误/日志序列化结果不含 key、bearer(含大小写变体)、URL。
- Compatibility: `HELIXFLOW_RUNTIME_PROVIDER` 语义从"唯一开关"变为"默认 id",未设置时行为不变(mock);GH-40 前端契约字段只增(`selectedProvider`、合成条目)不改。`image.mock.generate`/`video.mock.text_to_video` classType 删除是破坏性变更:存量 DB 中引用旧 classType 的图在新 build 下校验失败,发布说明需注明(产品未发布,owner 接受无兼容层)。
- Performance: registry 查表为 O(log n) 且 provider 数量个位数,可忽略;Atlas 轮询沿用 salvage 的 interval/timeout env 参数;产物下载/落盘在 run 执行线程内进行,与上游生成耗时同量级。
- Maintenance: gateway 引入 `reqwest` 增加编译面;换取 provider 实现集中于一个 crate,server 保持薄装配层。脱敏文案约束(不得含 `api_key` 子串)需用测试锁定,防止后续改文案时静默变成通用消息。

## 测试计划

- [ ] Unit tests: gateway — registry 分发(命中/未注册拒绝)、多 provider snapshot 聚合、Atlas `from_env`(有/无 key)、401 错误映射与消息脱敏、unavailable 原因文案通过 `is_safe_provider_message`。
- [ ] Unit tests: registry/graph — 节点定义 provider 中立(无 provider 绑定、label 不含 Mock);`compile_plan(graph, version_id, provider_id)` 把参数 id 写进每个 step。
- [ ] Unit tests: run — provider 返回字节落盘到 `artifacts/{run_id}/{step_index}.{ext}`、`storage_uri` 为相对路径且非 http(s);run 请求 `provider` 原样透传到 steps。
- [ ] Unit tests: store — migration 后 `runtime_provider_id` 读写、NULL 回退语义。
- [ ] Integration tests: server — `PUT /api/workspaces/{id}/provider`(合法/未注册/空 id)、选择持久化跨 Store 重开、unavailable provider 入队被拒(无产物)、queue 后切换选择不影响 in-flight run、workspace state 含 `selectedProvider`、持久化 id 未注册时合成 unavailable 条目、`GET /api/artifacts/{id}/content` 的 mime/404/路径穿越用例、`GET /api/registry/catalog` 响应 shape 不含 provider 字段。
- [ ] Integration tests: web — TopBar 选择器渲染双 provider 状态(含合成 unavailable 条目)、选择调用 API、gate 按 `selectedProvider` 判定(`app.test.tsx`,classType 同步更新为 `image.generate`/`video.text_to_video`)。
- [ ] Manual verification: 设 `ATLAS_API_KEY` 启动,选择 atlas 跑文生图 workflow,确认产出真实图片经 content 端点可预览/下载;去掉 key 重启,确认 catalog 标记不可用且运行被拒。

## 回滚方案

- 代码回滚:revert PR 即可恢复单 provider 装配与 mock classType;`RunService` 变更(provider 透传、落盘)随 revert 一并回退,mock 路径全程可用。
- 数据回滚:`workspaces.runtime_provider_id` 为可空列,旧代码(SELECT 显式列名)不读该列,留存无害;落盘的 `data_dir/artifacts/` 文件为纯附加数据,旧代码不读,可直接删除;无需 down migration。
- 运维开关:不设置 `ATLAS_API_KEY` 即可让 atlas 回到 unavailable 状态,系统整体退化为与今天等价的 mock-only 行为。
