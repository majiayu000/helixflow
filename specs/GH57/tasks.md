# Task Plan

## Linked Issue

GH-57

## Spec Packet

- Product: `specs/GH57/product.md`
- Tech: `specs/GH57/tech.md`

## 实现任务

- [ ] `SP57-T1` gateway:新增 `ProviderRegistry`(`crates/gateway/src/registry.rs`)并为 `RuntimeProvider` 扩展 `Atlas` variant;registry 实现 `Provider` 按 `req.provider` 分发,未注册 id 返回 `ProviderError::Unavailable`;snapshot 聚合全部注册 provider。Owner: gateway lane。Done when: registry 分发与多 provider snapshot 单测通过,未注册 id 明确拒绝无 fallback。Verify: `cargo test -p helixflow-gateway`
- [ ] `SP57-T2` gateway:从 `salvage/local-workbench-atlas-20260702:crates/server/src/provider.rs` 捞回 `AtlasProvider` 适配为 `crates/gateway/src/atlas.rs`(`from_env` 读 `ATLAS_API_KEY`/`ATLAS_API_BASE`,image_generate/chat_completion/text_to_video+轮询);新增 `ProviderError::RequestFailed` 并接入脱敏;无 key 时产出安全的 unavailable 原因文案(不含 `api_key`/`token` 子串)。Owner: gateway lane。Done when: 有/无 key 两种装配、401 错误映射脱敏、unavailable 文案通过 `is_safe_provider_message` 的单测全部通过。Verify: `cargo test -p helixflow-gateway`
- [ ] `SP57-T3` store:migration `crates/store/migrations/0002_workspace_runtime_provider.sql` 给 `workspaces` 加可空列 `runtime_provider_id`;`WorkspaceRecord` 增加字段;新增 `set_workspace_runtime_provider`。Owner: store lane。Done when: 写入选择后重开 Store 读回一致,NULL 表示未选择。Verify: `cargo test -p helixflow-store`
- [ ] `SP57-T4` server:`app_state.rs` 改为构造 `ProviderRegistry`(mock 恒注册,atlas 按 env 注册可用/不可用条目,`HELIXFLOW_RUNTIME_PROVIDER` 作为默认 id),`runner` 改为 `RunService<ProviderRegistry>`;`persist_runtime_provider_status` 遍历全部 provider。Owner: server lane。Done when: app_state 测试覆盖 mock 默认、atlas 无 key 不可用、未知默认 id 三种装配。Verify: `cargo test -p helixflow-server app_state`
- [ ] `SP57-T5` server:新增 `PUT /api/workspaces/{workspace_id}/provider` 路由(校验已注册 id,未注册返回 400 且原选择不变);workspace state `providers` 增加 `selectedProvider`;`queue_workspace_run` 入队时解析生效 provider 并固定到 run steps,unavailable 时 409/422 拒绝不产生产物。Owner: server lane。Done when: 选择路由、持久化、拒绝运行、运行中切换不影响 in-flight run 的集成测试全部通过。Verify: `cargo test -p helixflow-server`
- [ ] `SP57-T6` web:`top-bar.tsx` 增加 provider 选择器(列出 runtimeProviders 及状态徽标),`api.ts` 新增选择调用,`app.tsx` gate 改为按 `selectedProvider` 判定,`types.ts` 增加 `selectedProvider` 解析。Owner: frontend lane。Done when: 选择器渲染双 provider、选择触发 API、gate 用例通过且构建成功。Verify: `cd web && npm test -- app.test.tsx && npm run build`

## 并行拆分

三条 lane 文件所有权互不重叠,可并行;server lane 依赖 gateway/store 的接口先冻结(以本 spec 的契约为准,可先行按契约编码):

- gateway lane(SP57-T1、SP57-T2)只改:`crates/gateway/src/lib.rs`、`crates/gateway/src/runtime_provider.rs`、`crates/gateway/src/registry.rs`(新)、`crates/gateway/src/atlas.rs`(新)、`crates/gateway/Cargo.toml`。
- store lane(SP57-T3)只改:`crates/store/migrations/0002_workspace_runtime_provider.sql`(新)、`crates/store/src/lib.rs`、`crates/store/src/workspace_records.rs`。
- frontend lane(SP57-T6)只改:`web/src/components/top-bar.tsx`、`web/src/api.ts`、`web/src/app.tsx`、`web/src/types.ts`、`web/src/app.test.tsx`。
- server lane(SP57-T4、SP57-T5)在 gateway/store lane 合并后串行执行,只改:`crates/server/src/app_state.rs`、`crates/server/src/workspace_routes.rs`、`crates/server/src/workspace_state.rs`、`crates/server/src/run_routes.rs`、`crates/server/src/run_routes_unavailable_tests.rs`、`crates/server/src/main.rs`。
- 共享文件 `Cargo.lock` 由 coordinator 在合并时统一生成,任何 lane 不手改。

## 验证

- [ ] `SP57-T7` 全量回归:workspace 构建与测试全绿,格式检查通过。Owner: coordinator。Done when: 全部命令 exit 0 且输出记录进 PR。Verify: `cargo fmt --check && cargo check --workspace && cargo test --workspace && cd web && npm test && npm run build`
- [ ] `SP57-T8` spec 包校验与手动冒烟:SpecRail 校验通过;设 `ATLAS_API_KEY` 选择 atlas 跑文生图产出真实图片,去 key 重启确认 catalog 不可用且运行被拒。Owner: coordinator。Done when: check 脚本 exit 0,两个冒烟场景截图/日志附在 PR。Verify: `python3 checks/check_workflow.py --repo . --spec-dir specs/GH57`

## Handoff Notes

- 产品决策已由 owner 拍板,不得更改:workspace 级选择、凭据只读 env、无 key 明确拒绝禁止降级 mock(U-29)、本 issue 只交付框架 + Atlas,fal.ai 是 GH-61。
- 不做:计费、配额、节点级路由实现、BYOK UI、凭据热加载。
- 脱敏陷阱:`is_safe_provider_message`(crates/gateway/src/runtime_provider.rs:205 附近)会兜底替换含 `api_key`/`token`/`bearer` 等子串的消息,unavailable 原因文案要避开这些子串并用测试锁定;GH-55/GH-56 已修过 lowercase bearer,相关测试必须保留。
- 运行中切换语义:provider 在 queue 时固定到 run steps,in-flight run 不受切换影响,这是 P8 不变量,server lane 实现时不要改成运行期动态解析。
- capability id(`image_generate` 等)是跨 provider 共享词汇,不要为 atlas 引入另一套 capability 命名。
- 验收只卡文生图 e2e;atlas 的 chat_completion/text_to_video 捞回后有单测即可,不阻塞合并。
