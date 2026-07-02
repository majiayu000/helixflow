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
| Catalog API | `crates/server/src/main.rs:65`、`crates/server/src/registry_routes.rs` | `GET /api/registry/catalog` 返回 node registry + provider snapshot | catalog 需反映多 provider 各自健康状态 |
| Workspace state | `crates/server/src/workspace_state.rs` | state payload 含 `providers`(GH-40 契约);`node_provider()` 从 node registry 默认取 `mock` | 增加 `selectedProvider`;run 步骤的 provider 需按 workspace 选择解析 |
| Run 执行路径 | `crates/run/src/lib.rs`(`RunService<P>`,约 163-420 行) | `RunService` 泛型持单一 provider,`invoke` 用 `step.provider` 构造请求 | registry 实现 `Provider` 后可原样复用 `RunService<ProviderRegistry>`,按 `req.provider` 分发 |
| 持久化 | `crates/store/src/lib.rs`、`crates/store/migrations/0001_initial.sql`、`crates/store/src/workspace_records.rs` | `workspaces` 表(id/name/cur_version_id/时间戳);`providers` 表已有 `upsert_provider_status` | workspace 级选择的持久化位置;providers 表继续存健康状态 |
| 前端 gate 与展示 | `web/src/app.tsx:98-103`、`web/src/components/top-bar.tsx`、`web/src/types.ts`、`web/src/api.ts` | gate 只认 `defaultProvider` 对应条目 healthy 才允许 queue;TopBar 只读展示 provider 状态 | gate 改为看 workspace 已选 provider;TopBar 增加选择器 |
| 旧 Atlas 实现 | `git show salvage/local-workbench-atlas-20260702:crates/server/src/provider.rs` | 完整 `AtlasProvider`:chat_completion / image generate/edit / text_to_video + 轮询,`ATLAS_API_KEY`/`ATLAS_API_BASE` 读 env,bearer 鉴权 | 捞回适配为新 registry 下的 gateway 模块 |

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
- `AppState.runner` 从 `RunService<RuntimeProvider>` 改为 `RunService<ProviderRegistry>`,`RunService` 本身不改。
- 注册装配在 `app_state.rs`:
  - `mock` 恒注册(`RuntimeProvider::mock()`)。
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

### 5. Catalog 反映多 provider 健康状态

- `ProviderCatalogSnapshot` 结构不变(GH-40 契约),但由 registry 聚合生成:`runtime_providers` 列出全部注册 provider,每项独立 `enabled`/`status`/`message`/`capabilities`;`default_provider` = 服务器默认 id。
- atlas 可用时 `kind: "external_api"`、`status: "healthy"`;无 key 时 `kind: "unavailable"`、`enabled: false`。
- `persist_runtime_provider_status` 改为对 registry 内每个 provider 逐一 upsert 到 `providers` 表。
- workspace state 的 `providers` payload 增加一个 key:`selectedProvider`(string,已解析的生效 provider id;未选择时等于 `defaultProvider`)。

### 6. 运行路径:queue 时解析并固定 provider

- `queue_workspace_run` 在入队时解析生效 provider id(workspace 选择 → 默认),写入 run steps 的 `provider` 字段(替代现在从 node registry 默认取 `mock` 的来源);run 执行期间不再重新解析——运行中切换选择不影响 in-flight run。
- 入队前检查生效 provider 在 registry 中的健康状态:unavailable 则返回 409/422 明确错误,run 不入队,不产生任何产物(禁止降级,对齐 U-29)。
- capability id(`image_generate` 等)是跨 provider 共享词汇,mock 与 atlas 同名 capability 语义一致,node 定义无需按 provider 分叉。

### 7. Atlas provider(第一个真实 provider)

- 从 salvage 捞回:`from_env()` 读 `ATLAS_API_KEY`/`ATLAS_API_BASE`(默认 `https://api.atlascloud.ai/v1`)、bearer 鉴权、`image_generate` 同步生成、`text_to_video` 提交 + 轮询、`chat_completion`。本 issue 验收只卡 `image_generate` 端到端;其余 capability 捞回后以单测覆盖,不作为验收阻塞。
- `ProviderError` 新增上游失败 variant(如 `RequestFailed { provider, status, message }`);message 在进入任何日志/事件/API 响应前必须经 `is_safe_provider_message` 同源的脱敏处理。
- 注意:unavailable 原因文案不得包含 `api_key`/`token` 等子串(`is_safe_provider_message` 会把它兜底替换成通用文案),文案写成如 `Atlas credentials are not configured; set the Atlas environment before enabling this provider` 的形式,并加测试锁定。

### 8. 前端选择 UI:TopBar

- `web/src/components/top-bar.tsx` 增加 provider 下拉选择器:列出 `providers.runtimeProviders`,每项显示 label + 状态徽标(healthy/unavailable);当前值为 `providers.selectedProvider`。
- 选择触发 `PUT /api/workspaces/{id}/provider`(`web/src/api.ts` 新增函数),成功后刷新 workspace state。
- `web/src/app.tsx` 的 `providerReady` gate 从"`defaultProvider` 对应条目 healthy"改为"`selectedProvider` 对应条目 enabled 且 healthy"。
- 不做设置面板、不做凭据输入。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 多 provider catalog | `registry.rs` 聚合 snapshot、`registry_routes.rs` | `cargo test -p helixflow-gateway`;`cargo test -p helixflow-server workspace_state` |
| P2 无 key 标记不可用 | `atlas.rs::from_env`、app_state 装配 | gateway 单测:无 env 时 snapshot 断言 `enabled=false` 且 message 安全 |
| P3 不可用 provider 拒绝运行 | `run_routes.rs` 入队检查 | `cargo test -p helixflow-server run_routes`(扩展 unavailable 测试) |
| P4 选择持久化 | migration 0002、store、`PUT .../provider` | store 单测 + server 路由测试:写入后重开 Store 仍读回 |
| P5 未注册 id 被拒 | provider 选择路由校验 | server 路由测试断言 400 与选择不变 |
| P6 Atlas 文生图 e2e | `atlas.rs::invoke(image_generate)`、run 路径 | 手动验证:设 `ATLAS_API_KEY` 跑文生图 workflow,产物可下载 |
| P7 上游 401 明确失败且脱敏 | `atlas.rs` 错误映射、`ProviderError::RequestFailed` | gateway 单测(mock http/固定 401 响应体)断言消息无 bearer/key |
| P8 运行中切换不影响 in-flight run | queue 时固定 provider 到 run steps | server run 测试:入队后改选择,断言 run 仍用原 provider |
| P9 mock 与真实并存 | registry 装配、前端 gate | gateway snapshot 测试含双条目;web `app.test.tsx` gate 用例 |
| P10 凭据零泄漏 | 脱敏层复用(`is_safe_provider_message`) | 现有 redaction 测试保留 + 新增 atlas 消息脱敏用例 |

## 数据流

- 输入:环境变量 `ATLAS_API_KEY`、`ATLAS_API_BASE`(可选)、`HELIXFLOW_RUNTIME_PROVIDER`(默认 provider id);前端 `PUT /api/workspaces/{id}/provider` 的 `providerId`。
- 持久化:`workspaces.runtime_provider_id`(仅 provider id 字符串,绝不存凭据);`providers` 表存各 provider 健康状态快照。
- 输出:`GET /api/registry/catalog` 与 workspace state `providers`(含 `selectedProvider`);run events/artifacts 与现有链路一致。
- 外部调用:AtlasProvider 经 HTTPS 调 Atlas API(bearer 鉴权,key 仅存于进程内存中的 provider 配置结构);产物字节按现有 artifact 存储路径落盘。
- agent ctx:继续走 GH-40 的安全 catalog 投影,新增 atlas 条目时同样不含任何 env 值。

## 备选方案

- 节点级 provider 路由:`ProviderRequest.provider` 已按节点携带,未来可让单个节点覆盖 workspace 选择;本 issue 只做 workspace 级,避免 UI 与 run 语义复杂化(owner 已拍板)。
- 新建 `workspace_settings` 表存选择:更通用但引入 join 与第二张表的生命周期管理;当前只有一个设置项,直接加列到 `workspaces` 更简单,拒绝该方案。
- `Arc<dyn Provider>` 动态注册:更开放但丢失 Clone/Debug 且现阶段 provider 数量个位数;enum + registry 查表已满足"多注册、可插拔",拒绝过度设计。
- 凭据热加载(运行期重读 env):增加状态同步复杂度,启动时读取 + 重启生效已满足产品决策,列为未来可选。

## 风险

- Security: 凭据泄漏是最大风险。缓解:key 只进 provider 内存配置;所有对外文案过 `is_safe_provider_message` 同源脱敏;新增测试断言 catalog/错误/日志序列化结果不含 key、bearer(含大小写变体)、URL。
- Compatibility: `HELIXFLOW_RUNTIME_PROVIDER` 语义从"唯一开关"变为"默认 id",未设置时行为不变(mock);GH-40 前端契约字段只增(`selectedProvider`)不改,旧字段语义保持。
- Performance: registry 查表为 O(log n) 且 provider 数量个位数,可忽略;Atlas 轮询沿用 salvage 的 interval/timeout env 参数。
- Maintenance: gateway 引入 `reqwest` 增加编译面;换取 provider 实现集中于一个 crate,server 保持薄装配层。脱敏文案约束(不得含 `api_key` 子串)需用测试锁定,防止后续改文案时静默变成通用消息。

## 测试计划

- [ ] Unit tests: gateway — registry 分发(命中/未注册拒绝)、多 provider snapshot 聚合、Atlas `from_env`(有/无 key)、401 错误映射与消息脱敏、unavailable 原因文案通过 `is_safe_provider_message`。
- [ ] Unit tests: store — migration 后 `runtime_provider_id` 读写、NULL 回退语义。
- [ ] Integration tests: server — `PUT /api/workspaces/{id}/provider`(合法/未注册/空 id)、选择持久化跨 Store 重开、unavailable provider 入队被拒(无产物)、queue 后切换选择不影响 in-flight run、workspace state 含 `selectedProvider`。
- [ ] Integration tests: web — TopBar 选择器渲染双 provider 状态、选择调用 API、gate 按 `selectedProvider` 判定(`app.test.tsx`)。
- [ ] Manual verification: 设 `ATLAS_API_KEY` 启动,选择 atlas 跑文生图 workflow,确认产出真实图片可预览/下载;去掉 key 重启,确认 catalog 标记不可用且运行被拒。

## 回滚方案

- 代码回滚:revert PR 即可恢复单 provider 装配;`RunService` 未改动,mock 路径全程可用。
- 数据回滚:`workspaces.runtime_provider_id` 为可空列,旧代码(SELECT 显式列名)不读该列,留存无害;无需 down migration。
- 运维开关:不设置 `ATLAS_API_KEY` 即可让 atlas 回到 unavailable 状态,系统整体退化为与今天等价的 mock-only 行为。
