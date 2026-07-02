# Tech Spec

## Linked Issue

GH-61

## Product Spec

`specs/GH61/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Provider abstraction | `crates/gateway/src/lib.rs`, `crates/gateway/src/runtime_provider.rs` | `Provider` trait 已有 `catalog_snapshot` / `invoke` / `estimate`;当前 main 仍主要是 mock/unavailable 形态,GH-57 将引入多 provider registry | fal.ai 必须实现同一 provider 契约,不能绕过 run service |
| Server provider wiring | `crates/server/src/app_state.rs`, `crates/server/src/workspace_state.rs` | provider catalog 由 server 启动时构造并投影到 workspace state | `fal` 注册、可用性和默认选择必须进入同一状态面 |
| Run artifact contract | `crates/run/src/lib.rs`, `crates/server/src/artifact_routes.rs` | provider 输出通过 `ArtifactPayload` 进入 run,artifact 由服务端持久化并通过安全 content/preview 返回 | fal.ai remote URL 必须被服务端收敛为安全 artifact,不能裸露上游 URL |
| Cost and errors | `crates/run/src/cost_gate.rs`, `crates/gateway/src/runtime_provider.rs` | provider estimate 和错误会写入 run/cost 记录 | fal.ai 不支持精确估价时使用安全静态估算;认证/限流错误必须脱敏 |

## 设计方案

新增 `FalProvider` 作为 GH-57 `ProviderRegistry` 的一个注册项,provider id 固定为 `fal`。`FalProvider::from_env` 只读取 `FAL_KEY` 和可选 `FAL_API_BASE`;无 key 时仍注册 summary,但 `enabled=false`、`status=unavailable`。密钥只存在进程内配置,不写 DB、不进日志、不进 API response。

能力先支持 `image_generate`。实现把 Helixflow `ProviderRequest` 的 prompt/尺寸/seed 等参数映射到一个固定 fal.ai 文生图模型,提交 fal.ai 请求后轮询到终态。成功时把图片 bytes 或 HTTPS output URL 转为 `ArtifactPayload` 的图片 artifact,交给 RunService 统一落盘。失败时把 401/403 映射为鉴权失败,429 映射为限流,其他上游失败映射为 `RequestFailed`,所有消息走现有脱敏 helper。

provider catalog 中 `fal` 的 capabilities 只列出已实现能力。若图中请求 `text_to_video` 等未实现 capability,registry/provider 返回明确 unsupported,RunService 将 run 收敛为 failed,不得 fallback 到其他 provider。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1、P2 | gateway/server provider 注册和 workspace state 投影 | server 单测:无 `FAL_KEY` 时 `fal` unavailable 且原因脱敏 |
| P3、P7 | provider resolve + RunService 错误收敛 | run/server 测试:选择 unavailable/unsupported fal 能明确失败且无产物 |
| P4、P5 | `FalProvider::invoke(image_generate)` + artifact 持久化 | gateway fake HTTP 测试 + server 集成测试:同图切换 provider 后产出图片 |
| P6 | 错误脱敏 | gateway 单测:401/403/429/raw bearer 响应不泄露 `FAL_KEY` |

## 数据流

启动读取 `FAL_KEY` -> 注册 `fal` summary -> workspace 选择 `fal` -> run step 以 provider `fal` 和 capability `image_generate` 调用 `FalProvider` -> fal.ai queue/poll -> `ArtifactPayload` -> RunService 写 artifact -> 前端通过 workspace state/content URL 预览。

## 备选方案

- 把 fal.ai 做成 Atlas adapter 的分支:被否,会把 provider 抽象重新特化到单实现。
- 直接把 fal.ai remote URL 暴露到前端:被否,破坏 artifact 安全契约和可重启预览。

## 风险

- Security: `FAL_KEY` 是密钥,只读 env,错误和日志必须脱敏。
- Compatibility: 依赖 GH-57 provider id 和 provider-neutral node type;GH-57 未实现前不能落地。
- Performance: fal.ai 队列轮询需超时和取消,避免 run 卡死。
- Maintenance: 固定模型映射要集中在 provider 配置,后续扩展模型目录不要散落到 UI。

## 测试计划

- [ ] Unit tests: `FalProvider::from_env`、capability summary、无 key unavailable、401/403/429 脱敏、unsupported capability。
- [ ] Integration tests: 以 fake fal.ai server 跑 `image.generate`,断言 artifact content URL 可预览且 `storage_uri` 非上游 URL。
- [ ] Manual verification: 设置 `FAL_KEY`,本地选择 `fal` 跑文生图;去掉 key 重启后确认 unavailable 和运行拒绝。

## 回滚方案

关闭或移除 `fal` 注册即可回到 GH-57 的其他 provider;无 schema 迁移时可直接 revert。若已写入 workspace 选择 `fal`,降级后 workspace state 应显示合成 unavailable 条目。
