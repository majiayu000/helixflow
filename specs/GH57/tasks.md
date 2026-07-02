# Task Plan

## Linked Issue

GH-57

## Spec Packet

- Product: `specs/GH57/product.md`
- Tech: `specs/GH57/tech.md`

## 实现任务

- [ ] `SP57-T1` gateway:新增 `ProviderRegistry`(`crates/gateway/src/registry.rs`)并为 `RuntimeProvider` 扩展 `Atlas` variant;registry 实现 `Provider` 按 `req.provider` 分发,未注册 id 返回 `ProviderError::Unavailable`;snapshot 聚合全部注册 provider,mock 条目 label 标识为本地测试 provider。Owner: gateway lane。Done when: registry 分发与多 provider snapshot 单测通过,未注册 id 明确拒绝无 fallback。Verify: `cargo test -p helixflow-gateway`
- [ ] `SP57-T2` gateway:捞回 `AtlasProvider` 适配为 `crates/gateway/src/atlas.rs`——来源用可复现命令 `git fetch origin salvage/local-workbench-atlas-20260702 && git show FETCH_HEAD:crates/server/src/provider.rs`(commit `4f3e5ea`,分支已在 origin);`from_env` 读 `ATLAS_API_KEY`/`ATLAS_API_BASE`,image_generate/chat_completion/text_to_video+轮询;新增 `ProviderError::RequestFailed` 并接入脱敏;无 key 时产出安全的 unavailable 原因文案(不含 `api_key`/`token` 子串)。Owner: gateway lane。Done when: 有/无 key 两种装配、401 错误映射脱敏、unavailable 文案通过 `is_safe_provider_message` 的单测全部通过。Verify: `cargo test -p helixflow-gateway`
- [ ] `SP57-T3` store:migration `crates/store/migrations/0002_workspace_runtime_provider.sql` 给 `workspaces` 加可空列 `runtime_provider_id`;`WorkspaceRecord` 增加字段;新增 `set_workspace_runtime_provider`。Owner: store lane。Done when: 写入选择后重开 Store 读回一致,NULL 表示未选择。Verify: `cargo test -p helixflow-store`
- [ ] `SP57-T9` registry/graph/run:节点定义 provider 中立化——`crates/registry/src/lib.rs` 删除 `image.mock.generate`/`video.mock.text_to_video`,改为 `image.generate`/`video.text_to_video`(label 不含 Mock,capability 保留,provider 绑定移除,含 `llm.prompt_writer`;estimated_cost key 改按 capability);`GraphService::compile_plan` 增加 `provider_id: &str` 参数写入每个 `ExecutionStep.provider`;`ManualRunRequest`/`AgentRunRequest` 增加必填 `provider: String`,`execute_manual_run`/`request_agent_run` 原样透传给 `compile_plan`;本 lane 内引用旧 classType 的测试(`crates/graph/src/tests.rs`、`crates/run/src/tests.rs`、`crates/agent/src/tests.rs`)同步更新。Owner: graph/run lane。Done when: registry 单测断言节点无 provider 绑定且 label 不含 Mock;graph 单测断言参数 id 写进全部 steps;run 单测断言请求 provider 原样落到 run steps;旧 classType 在本 lane 无残留引用。Verify: `cargo test -p helixflow-registry -p helixflow-graph -p helixflow-run -p helixflow-agent`
- [ ] `SP57-T10` run:产物字节落盘契约——`RunService` 构造时注入 artifacts 根目录(`data_dir/artifacts`),把 provider 返回的字节(或先下载其 HTTPS URL 内容)写入 `artifacts/{run_id}/{step_index}.{ext}`(ext 由 mime 推导);`persist_artifact` 的 `storage_uri` 存该相对路径,绝不存上游 URL;mock 产物迁移到同一契约。Owner: graph/run lane。Done when: run 单测断言文件落盘、`storage_uri` 为相对路径且非 http(s)。Verify: `cargo test -p helixflow-run`
- [ ] `SP57-T4` server:`app_state.rs` 改为构造 `ProviderRegistry`(mock 恒注册,atlas 按 env 注册可用/不可用条目,`HELIXFLOW_RUNTIME_PROVIDER` 作为默认 id),`runner` 改为 `RunService<ProviderRegistry>` 并注入 artifacts 根目录;`persist_runtime_provider_status` 遍历全部 provider。Owner: server lane。Done when: app_state 测试覆盖 mock 默认、atlas 无 key 不可用、未知默认 id 三种装配。Verify: `cargo test -p helixflow-server app_state`
- [ ] `SP57-T5` server:新增 `PUT /api/workspaces/{workspace_id}/provider` 路由(校验已注册 id,未注册返回 400 且原选择不变);workspace state `providers` 增加 `selectedProvider`,持久化 id 未注册时合成 unavailable 条目(原因文案避开 `api_key`/`token`/`bearer` 子串)且不改写选择;`queue_workspace_run` 与 agent run 入口在入队时解析生效 provider(workspace 选择 → NULL 用服务器默认)填入 run 请求 `provider` 字段,unavailable 时 409/422 拒绝不产生产物;`GET /api/registry/catalog` 保持纯节点 catalog 不动;server 内引用旧 classType 的测试与内置图同步更新为新 classType。Owner: server lane。Done when: 选择路由、持久化、拒绝运行、运行中切换不影响 in-flight run、合成 unavailable 投影、catalog 响应 shape 无 provider 字段的集成测试全部通过。Verify: `cargo test -p helixflow-server`
- [ ] `SP57-T11` server:产物内容端点——新增 `GET /api/artifacts/{artifact_id}/content`,按 artifacts 行 `mime` 设 Content-Type 回源 `data_dir` 下文件字节,路径规范化后必须落在 artifacts 根目录内(防穿越),文件缺失 404;`safe_download_uri` 与 preview/download 改为指向该端点,保留"不暴露原始 storage_uri"的既有测试语义。Owner: server lane。Done when: content 端点 mime/404/路径穿越用例与 preview/download 回归全部通过。Verify: `cargo test -p helixflow-server artifact`
- [ ] `SP57-T6` web:`top-bar.tsx` 增加 provider 选择器(数据源为 workspace state 的 `providers`,列出 runtimeProviders 及状态徽标,含合成 unavailable 条目),`api.ts` 新增选择调用,`app.tsx` gate 改为按 `selectedProvider` 判定,`types.ts` 增加 `selectedProvider` 解析;`app.test.tsx` 中旧 classType 引用同步更新为 `image.generate`/`video.text_to_video`。Owner: frontend lane。Done when: 选择器渲染双 provider、选择触发 API、gate 用例通过且构建成功。Verify: `cd web && npm test -- app.test.tsx && npm run build`

## 并行拆分

四条 lane 文件所有权互不重叠,可并行;server lane 依赖 gateway/store/graph-run 的接口先冻结(以本 spec 的契约为准,可先行按契约编码):

- gateway lane(SP57-T1、SP57-T2)只改:`crates/gateway/src/lib.rs`、`crates/gateway/src/runtime_provider.rs`、`crates/gateway/src/registry.rs`(新)、`crates/gateway/src/atlas.rs`(新)、`crates/gateway/Cargo.toml`。
- store lane(SP57-T3)只改:`crates/store/migrations/0002_workspace_runtime_provider.sql`(新)、`crates/store/src/lib.rs`、`crates/store/src/workspace_records.rs`。
- graph/run lane(SP57-T9、SP57-T10)只改:`crates/registry/src/lib.rs`、`crates/graph/src/lib.rs`、`crates/graph/src/tests.rs`、`crates/run/src/lib.rs`、`crates/run/src/cost_gate.rs`、`crates/run/src/tests.rs`、`crates/agent/src/tests.rs`。
- frontend lane(SP57-T6)只改:`web/src/components/top-bar.tsx`、`web/src/api.ts`、`web/src/app.tsx`、`web/src/types.ts`、`web/src/app.test.tsx`。
- server lane(SP57-T4、SP57-T5、SP57-T11)在 gateway/store/graph-run lane 合并后串行执行,只改:`crates/server/src/app_state.rs`、`crates/server/src/workspace_routes.rs`、`crates/server/src/workspace_state.rs`、`crates/server/src/run_routes.rs`、`crates/server/src/run_routes_unavailable_tests.rs`、`crates/server/src/artifact_routes.rs`、`crates/server/src/main.rs`,以及 server crate 内其余引用旧 classType 的测试文件(`version_routes.rs`、`proposal_routes.rs`、`layout_routes.rs`、`sweep_support.rs`、`manual_proposal_routes_tests.rs`、`workbench_message.rs`)。
- 共享文件 `Cargo.lock` 由 coordinator 在合并时统一生成,任何 lane 不手改。

## 验证

- [ ] `SP57-T7` 全量回归:workspace 构建与测试全绿,格式检查通过,旧 classType(`image.mock.generate`/`video.mock.text_to_video`)在仓库内(specs/ 除外)零残留。Owner: coordinator。Done when: 全部命令 exit 0 且输出记录进 PR。Verify: `cargo fmt --check && cargo check --workspace && cargo test --workspace && cd web && npm test && npm run build`
- [ ] `SP57-T8` spec 包校验与手动冒烟:SpecRail 校验通过;设 `ATLAS_API_KEY` 选择 atlas 跑文生图产出真实图片(经 `GET /api/artifacts/{id}/content` 预览/下载),去 key 重启确认 catalog 不可用且运行被拒。Owner: coordinator。Done when: check 脚本 exit 0,两个冒烟场景截图/日志附在 PR。Verify: `python3 checks/check_workflow.py --repo . --spec-dir specs/GH57`

## Handoff Notes

- 产品决策已由 owner 拍板,不得更改:workspace 级选择、凭据只读 env、无 key 明确拒绝禁止降级 mock(U-29)、本 issue 只交付框架 + Atlas,fal.ai 是 GH-61。
- 不做:计费、配额、节点级路由实现、BYOK UI、凭据热加载、新的 provider 目录端点(`GET /api/registry/catalog` 保持纯节点 catalog,provider 状态只走 workspace state)。
- Atlas 实现来源必须可复现:`git fetch origin salvage/local-workbench-atlas-20260702 && git show FETCH_HEAD:crates/server/src/provider.rs`(commit `4f3e5ea`);不要依赖本地已有该分支。
- 脱敏陷阱:`is_safe_provider_message`(crates/gateway/src/runtime_provider.rs:205 附近)会兜底替换含 `api_key`/`token`/`bearer` 等子串的消息,unavailable 原因文案(含合成条目的 reason)要避开这些子串并用测试锁定;GH-55/GH-56 已修过 lowercase bearer,相关测试必须保留。
- provider 注入链:server 入口解析(选择 → 默认)→ run 请求 `provider` 字段 → `compile_plan(graph, version_id, provider_id)` → run steps。RunService 不做解析,节点定义不再携带 provider;in-flight run 不受切换影响是 P8 不变量,不要改成运行期动态解析。
- classType 变更无兼容层(U-24):`image.mock.generate`/`video.mock.text_to_video` 删除,各 lane 只更新自己所属文件内的引用,coordinator 在 T7 做零残留检查。
- 产物契约:字节一律落盘 `data_dir/artifacts/{run_id}/{step_index}.{ext}`,`storage_uri` 存相对路径,绝不存上游 URL;content 端点必须做根目录约束。
- capability id(`image_generate` 等)是跨 provider 共享词汇,不要为 atlas 引入另一套 capability 命名。
- 验收只卡文生图 e2e;atlas 的 chat_completion/text_to_video 捞回后有单测即可,不阻塞合并。
