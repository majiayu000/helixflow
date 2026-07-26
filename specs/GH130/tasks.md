# Task Plan

## Linked Issue

GH-130

## Spec Packet

- Product: `specs/GH130/product.md`
- Tech: `specs/GH130/tech.md`

## 实现任务

- [x] `SP130-T0` fixtures 与 regression 基线：固化 Nano Banana/Seedance、GPT Image/Seedance、串行、并行、缺图、model mismatch 六类 fixtures；为现有 Agent→proposal 行为补 regression tests，作为切换前基线。Owner: coordinator。Done when: fixtures 能稳定复现 tech.md「Codebase Context」中的静默默认模型缺口，regression tests 全绿。Verify: `cargo test --workspace`
- [x] `SP130-T1` Catalog V2 与 resolver：`CapabilityDefinition`/`ModelDefinition`/`ConnectorDefinition`/`CapabilityBinding`/revision/availability 数据模型与持久化；`CapabilityResolver`（unknown/ambiguous/unavailable/pinned/policy 全分支）；catalog API（`GET /api/catalog`、`POST /api/catalog/resolve` 等）；V1 种子 catalog（atlas: nano-banana-2、seedance-v1.5-pro；fal: nano-banana-2）。Owner: catalog lane。Done when: 任意模型组合由 binding 数据表达；resolver 对 pinned/policy 结果确定且 many-to-many 单测通过。Verify: `cargo test -p helixflow-registry -p helixflow-gateway -p helixflow-store`
- [x] `SP130-T2` Graph V2 与 migration：`NodeSemantics`/`ImplementationSelection` 字段、graph validator 扩展（binding 一致性、params schema、pinned identity）、v1→v2 migrator（dry-run + report + `needs_resolution`，不从 title 推断模型）。Owner: graph lane。Done when: title/坐标/未声明 param 无法影响模型；迁移不改变 topology 且幂等。Verify: `cargo test -p helixflow-graph`
- [x] `SP130-T3` IntentPlan 与 compiler：`out/intent.json` contract 与 schema validation；新增 `crates/compiler`（intent/resolver/topology/graph_builder/proposal_diff/layout/errors）；clarify handoff；`POST /api/workflows/compile-intent`。Owner: compiler lane。Done when: 相同 intent + catalog + current graph 产生相同 proposal；Agent 不再输出低层图；六类澄清场景返回 `clarify_first`。Verify: `cargo test -p helixflow-compiler -p helixflow-agent`
- [x] `SP130-T4` ResolvedExecutionPlan 与 run preflight：`ResolvedExecutionStep` 不可变快照持久化；10 步 preflight；删除 `crates/gateway/src/atlas.rs:242`/`:269` 与 `crates/gateway/src/fal.rs:14` 的默认模型分支；审计字段写入 run trace。Owner: run lane。Done when: pinned 模型不一致时 run 创建失败（`PINNED_MODEL_MISMATCH`）；provider 代码零隐式默认模型。Verify: `cargo test -p helixflow-run -p helixflow-gateway -p helixflow-server`
- [ ] `SP130-T5` Workbench UX：capability/model 双视图 node library、implementation inspector（requested/resolved model、binding revision、不可运行原因）、澄清面板、proposal topology 预览。Owner: frontend lane。Done when: 用户能区分 capability、model、mode、connector 和 topology；澄清不伪装成功。Verify: `cd web && npx tsc --noEmit && npm test`
- [ ] `SP130-T6` 切换默认路径与清理 legacy：灰度启用 intent 路径、停止低层 Agent proposal 输出、移除 `params.model` 与运行期 `image_generate` 兼容、迁移存量图。Owner: coordinator。Done when: 全部 feature flag 默认开启、legacy 写入路径删除、存量图迁移完成或明确隔离、回滚演练通过。Verify: `cargo test --workspace && cd web && npm test`

## 并行拆分

T0 完成后，T1（后端 catalog）与 T5 的纯 UI 骨架（静态双视图，不接 API）可并行；
其余阶段有依赖顺序 T1→T2→T3→T4→T6。文件所有权互不重叠：

- catalog lane（SP130-T1）只改：`crates/registry/**`、`crates/gateway/**`、`crates/store/**`（catalog 记录与 migration）。
- graph lane（SP130-T2）只改：`crates/graph/**`、`crates/server/src/version_*`、migration 工具。
- compiler lane（SP130-T3）只改：`crates/agent/**`、`crates/compiler/**`（新建）、`crates/server` 的 proposal/compile 路由。
- run lane（SP130-T4）只改：`crates/run/**`、`crates/gateway/src/atlas.rs`、`crates/gateway/src/fal.rs`、`crates/server` 的 preflight 路由。
- frontend lane（SP130-T5）只改：`web/src/**`。
- 共享文件 `Cargo.lock` 由 coordinator 在合并时统一生成，任何 lane 不手改。
- 同一 tranche 内并行 agent 必须再按上述目录细分不重叠的文件集。

## 验证

- [ ] `SP130-T7` 全量回归：workspace 构建与测试全绿；provider 代码中默认模型字符串（`DEFAULT_FAL_IMAGE_MODEL`、`unwrap_or_else` 模型回退）零残留（specs/ 除外）。Owner: coordinator。Done when: 全部命令 exit 0 且输出记录进 PR。Verify: `cargo fmt --check && cargo check --workspace && cargo test --workspace && cd web && npx tsc --noEmit && npm test && npm run build`
- [ ] `SP130-T8` spec 包校验与手动冒烟：SpecRail 校验通过；Workbench 中完成 product.md 验收场景（Nano Banana→Seedance 串行、显式并行、缺图澄清），inspector 展示 requested/resolved model 一致。Owner: coordinator。Done when: check 脚本 exit 0，冒烟证据附在 PR。Verify: `python3 checks/check_workflow.py --repo . --spec-dir specs/GH130`
- [ ] `SP130-T9` 对照 product.md 验收标准逐项打勾，每项引用当次会话内的命令输出，不引用历史输出。Owner: coordinator。Done when: 验收清单全部勾选并附证据。Verify: `python3 checks/check_workflow.py --repo . --all-specs`

## Handoff Notes

- T1 实现决策：catalog 为代码内种子数据（`crates/registry/src/catalog_seed.rs`），
  revision 为内容 sha256；catalog 的数据库持久化推迟到 T4 随 run snapshot 一起落
  （run 创建时固化 immutable snapshot，P11/P12），避免在没有写入方的阶段建表。
- T1 实现决策：`ModelDefinition` 增加 `aliases` 字段（spec §3.2 的确定性补充——
  用户口语名必须解析且禁止子串匹配，需要显式别名数据）；"seedance 2" 作为
  `bytedance/seedance-v1.5-pro` 的别名录入（与 T0 fixtures 的 utterance 对齐）。
- T2 实现决策：V2 采用分层结构 `WorkflowGraphV2 = v1 base（结构层，字节级兼容）+
  semantics map（语义层）`，而非立即把 `semantics/implementation` 嵌入
  `GraphNode`——全仓约 130 处 struct 字面量构造点在 v1 存活期内不做两次机械改动，
  嵌入在 T6 删除 v1 写路径时一并完成。
- T2 实现决策：迁移时无声明模型的节点转成显式 `Policy(capability_default)`，只能
  经配置的默认 binding 解析（P5），与"采用 provider 内部默认值"有本质区别；
  `needs_resolution` 保留给 capability 缺失、无默认 binding、模型不可解析三类。
- T4 实现决策：实现快照直接冻结在 `runs.plan_json`（`ExecutionStep.resolved` +
  `ExecutionPlan.catalog_revision`）——它本就是 retry/self-heal/sweep 复用的不可变
  run 级事实源（P11/P12），无需新表。
- T4 实现决策：解析发生在 run 创建的 compile 之后（`attach_resolved_bindings`），
  catalog 外的 provider（mock）跳过；resolver 新增 `connector_preference`
  （spec §5 backend_preference）——有偏好时默认 binding 不在该 connector 上则唯一
  候选即显式选择。preflight 与 run 冻结走同一解析函数，杜绝两者漂移。
- T4 实现决策：cache key schemaVersion 2→3，纳入 `step.resolved`——模型身份从
  provider config 移到 step 后，缓存不可再跨模型串用（HF-021 延伸）。
- T4 完成：atlas 三处（chat/image/video）与 fal 的静默默认模型全部删除，
  `ProviderRequest` 增加 `resolved_model_id`/`operation_id`，缺失即
  `ModelUnresolved` 显式错误；`run.resolved_implementations` 审计事件带
  requested/resolved model、binding、connector、catalogRevision。
- T3 实现决策：compile 把 create/extend/replace 统一为"当前图收敛到编译目标"的
  最小 diff（空图即纯创建）；`POST /api/workflows/compile-intent` 接受可选
  baseGraph，workspace 级集成随 T6 切换。
- T3 实现决策：required text 输入无上游且给了字面值时，编译器合成 `input.text`
  节点（Agent 只给值，结构由编译器物化）；v1 结构契约要求的 required param 在被
  连线满足时填空串占位并出 diagnostic，executor 运行时优先取连线值。
- T3 实现决策：resolver 的 recoverable 错误（ambiguous/unavailable）→
  `clarify_first`；MODEL/BINDING/CAPABILITY_NOT_FOUND → 硬错误（对齐
  intent-model-mismatch fixture）。golden catalog 在测试内以纯数据追加
  gpt-image-2 与 seedance image_to_video binding，证明零代码接模型。
- T3 修正：intent-serial-chain fixture 的 s1 params 键 prompt→text（对齐
  prompt_writer 输入契约）；registry 新增 `video.image_to_video` 内建节点；
  deepseek binding defaults 补 style=plain。
- T2 发现：`atlas.rs:195` 存在第三处静默默认模型（chat/prompt_writer →
  deepseek-ai/DeepSeek-V3-0324），已补入种子 catalog（model + binding + default），
  SP130-T4 与另外两处一并删除。
- T1 实现决策：pinned 模型命中多个 binding（nano-banana 同时有 atlas/fal）时，
  capability 级 `default_bindings` 优先破平，其次唯一可用者，否则
  `BINDING_AMBIGUOUS`（P5 的确定性延伸，见 `resolver.rs::select_unique`）。

- 维护者待决（spec 审批时确认）：V1 canonical model/binding 清单；policy
  selection 是否仅唯一默认 binding；v1 迁移截止版本。
- ComfyUI backend 已明确移出本期（维护者决定，2026-07-26），领域模型保留
  `WorkflowTemplate` 抽象位，后续另立 issue。
- 静默默认模型的三处代码证据：`crates/gateway/src/atlas.rs:242`、`:269`、
  `crates/gateway/src/fal.rs:14`，SP130-T4 删除。
- 原始草案 `specs/capability-driven-workflow-compiler.md` 已迁入本 packet 并删除。
