# Product Spec

## Linked Issue

GH-130

## 用户问题

用户在聊天里点名模型（如"用 Nano Banana 生成一张图，再用 Seedance 2 做成视频"）后，画布上显示的模型和实际执行的模型可能不一致：Agent 可能只改了节点 title 或写入未声明的参数，graph 校验无法验证模型绑定，provider 在参数缺失时静默使用内部默认模型（`crates/gateway/src/atlas.rs:242`、`crates/gateway/src/fal.rs:14`）。用户以为跑的是 A 模型，实际跑的是 B 模型，且事后无法从 run 记录中审计出差异。

同时，每支持一个新模型组合都需要修改 provider 代码，模型上新速度受代码发布节奏限制。

## 目标

- 用户点名的模型就是实际执行的模型；无法满足时显式失败，绝不静默替换。
- Agent 只表达高层意图（阶段、能力、模型、输入来源、串并行），图的连线、ID、布局由系统确定性生成。
- 新模型通过 catalog 数据上架即可被 Agent 编排，无需改代码。
- 每次 run 可审计：记录实际使用的 capability、model、binding 及其版本。

## 非目标

- ComfyUI workflow backend（维护者已决定另立 issue；本期仅保留抽象位）。
- 模型市场、自动下载 checkpoint、跨 provider 智能排名、质量自动评测。
- `loop` / `condition` / `human-input` 控制流。
- capability 缺失时自动"相近能力替换"。

## Behavior Invariants

1. 用户点名模型时，画布 inspector、run trace 与实际 provider 调用展示同一个 canonical model；三者不可能不一致，不一致的 run 无法被创建。
2. 用户未点名模型时，系统只使用明确配置的默认选择；无默认或多个候选时，Agent 回复澄清问题而不是替用户猜。
3. 用户要求串行时，产生的图是一条语义主链；并行分支只在用户明确表达并行意图时出现。
4. 节点的必需输入无法由用户输入或上游 typed 输出满足时，Agent 回复澄清问题，不生成残缺或错误拓扑的图。
5. 修改节点 title 或拖动节点位置不改变任何执行语义。
6. 新模型在 catalog 上架后即可被 Agent 编排和手动添加，无需等待代码发布。
7. 每个 run 创建时固化其 catalog、binding 与 resolved model 快照；run 创建后的 catalog 变更不影响该 run 的展示与重放解读。
8. 任何 capability、model、输入或连接器问题都产生带稳定 code 的显式错误或澄清，不出现 warning + fallback 的静默降级。
9. 旧版（v1）图迁移后节点与连线拓扑不变；无法唯一确定模型的节点被标记为"需要用户解决"，而不是被填入默认模型。
10. 错误信息与 Agent 上下文不包含密钥、内部 endpoint 或 provider 原始响应。
11. 澄清状态在 UI 中呈现为澄清，不伪装成成功的 proposal。

## 验收标准

- [ ] GPT Image + Seedance 且无 image 输入时返回澄清，不生成并行 text_to_video 拓扑。
- [ ] 用户要求串行时不产生语义 fan-out；只有明确并行意图才生成分支。
- [ ] pinned 模型 requested/resolved 不一致时 run 创建失败；provider 代码中不再存在隐式默认模型。
- [ ] Nano Banana ↔ GPT Image 互换只改 catalog 数据，不改代码分支。
- [ ] 一个模型可实现多个 capability，一个 capability 可由多个模型实现。
- [ ] 相同 intent + catalog + current graph 产生逐字节相同的 proposal。
- [ ] v1 graph dry-run 迁移不从 title 推断模型、不改变已有 topology。

## 边界情况

- 模型名歧义："Seedance" 命中多个 canonical model 时必须澄清，不能选"最接近"的。
- binding 存在但 connector 下线：preflight 失败并给出不可运行原因，不自动换模型。
- catalog revision 在 compile 与 run 创建之间变化：run 被阻止并要求 re-resolve。
- 用户同一句话中混合冲突的串行与并行表达：澄清。
- 迁移的 v1 图中存在未声明的 `params.model`：仅当该值可唯一映射到 canonical model 时采用，否则标记 `needs_resolution`。

## 发布说明

- 按 feature flag 灰度：`catalog_v2` → `graph_schema_v2` → `intent_plan_agent_contract` + `workflow_compiler` → `resolved_execution_plan` → `workbench_implementation_browser`，先双读/影子编译对比，再切默认路径，最后移除 legacy 写入。
- 回滚只切回兼容读取/旧执行路径，不覆盖已持久化的 v2 图。
- 待维护者在 spec 审批时确认的决策点：
  1. V1 canonical model/binding 清单（建议以现有真实调用为种子：`google/nano-banana-2`、`bytedance/seedance-v1.5-pro`、fal `nano-banana-2`）。
  2. policy selection V1 是否仅允许 workspace 唯一默认 binding（建议：是）。
  3. v1 图迁移截止与 legacy read path 删除版本（建议：最后一个 tranche 合并后两个 release）。
