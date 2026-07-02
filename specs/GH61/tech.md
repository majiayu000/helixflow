# Tech Spec

## Linked Issue

GH-61

## Product Spec

`specs/GH61/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Provider framework | `crates/provider-gateway`, `crates/server/src/provider*`, `crates/run/src/lib.rs` | GH-57 定义 provider registry、selection、artifact bytes/URL persistence | fal.ai 必须作为第二个 provider 接入该框架 |
| Workspace state | `crates/server/src/workspace_state.rs`, `workbench_payload.rs` | provider snapshot / selected provider 暴露给前端 | 需要展示 fal.ai available/unavailable |
| Artifact preview | `crates/server/src/artifact*`, `web/src/components/artifact-stage*` | 外部 provider artifact 需转为可预览 content URL | fal.ai image 输出必须复用 |
| Config/secrets | server startup/env config | 外部 provider key 来自 env | `FAL_KEY` 不得硬编码或泄露 |

## 设计方案

### 1. Provider 注册

在 provider registry 增加 `fal` provider id,声明至少 `image_generate` 能力。provider descriptor 包含 display name、capabilities、availability reason。无 `FAL_KEY` 时注册为 unavailable,但仍出现在 provider snapshot 中。

### 2. fal.ai 调用

新增 fal provider 实现,从 env 读取 `FAL_KEY`,用 HTTP client 调用一个固定代表模型的文生图 API。请求参数由现有 provider-neutral prompt/size/seed 映射而来;不支持的字段走 capability/validation error。同步/queue 形态由 provider 内部封装,对 `Provider::invoke` 只返回 `ArtifactPayload` 所需的 image bytes 或 remote URL。

### 3. 估算与错误

若 fal.ai 没有稳定公开 pricing endpoint,`estimate` 返回保守静态估算并标记 source=`static`,不得因为无法估算阻断 agent confirm。HTTP 401/403、timeout、non-2xx、malformed JSON、非 image result 都转换为 provider error,run failed,错误文本不得包含 key。

### 4. Artifact persistence

fal provider 不直接写数据库。它返回 provider artifact content,由 GH-57 的 `RunService` artifact persistence 写入 `data_dir/artifacts/...` 并生成最终 `storage_uri` / preview content URL。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 fal.ai 端到端出图 | fal provider + RunService artifact persistence | mocked fal HTTP e2e: image bytes/URL -> artifact preview payload |
| P2 切 provider 不改图 | provider selection path | 同一 graph,selected provider atlas/fal 产生不同 provider request |
| P3 无 key unavailable | registry/workspace state | env unset test asserts unavailable reason and run rejection |
| P4 capability gate | provider registry | unsupported capability returns provider/capability error |
| P5 外部失败可诊断 | fal provider error mapping | 401/timeout/malformed response unit tests |
| P6 secret 不泄露 | logs/errors/payloads | assert responses and debug strings do not contain key |

## 数据流

workspace selected provider -> run request resolves `fal` -> provider gateway invokes fal.ai with `FAL_KEY` -> provider returns image content/URL -> RunService persists artifact -> workspace state/preview exposes safe content URL.

## 备选方案

- 只把 fal.ai 作为 Atlas adapter 内的模型选项:会隐藏 provider 可插拔性,放弃。
- 前端直接调用 fal.ai:泄露 secret 且绕过 run/artifact 状态机,放弃。

## 风险

- Security: `FAL_KEY` 必须只从 env/secret manager 读取,不得进入 logs。
- Compatibility: fal.ai 模型参数与 Atlas 不完全一致,首版只映射 provider-neutral 最小参数。
- Performance: queue/poll 可能慢,需沿用后台 run 和 interrupt。
- Maintenance: fal.ai API 变更需隔离在 provider 实现内。

## 测试计划

- [ ] Unit tests: provider registration、availability、estimate、error mapping。
- [ ] Integration tests: mock fal HTTP server + run artifact persistence。
- [ ] Manual verification: 设置真实 `FAL_KEY` 后跑文生图 smoke test。

## 回滚方案

移除或禁用 fal provider registration;已生成 artifacts 保持可读,workspace provider selection 回退为 unavailable。

