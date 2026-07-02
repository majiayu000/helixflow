# Tech Spec

## Linked Issue

GH-60

## Product Spec

`specs/GH60/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| 意图分类器 | `crates/agent/src/turn_mode.rs` | `classify_turn_mode` 关键词匹配返回 `TurnMode`;无关键词命中时返回 `TurnRoutingError::Ambiguous` | 兜底逻辑的落点 |
| 消息入口 | `crates/server/src/workbench_message.rs` | `:59` 调用分类器,`Err` 一律映射 400;`:267` `turn_metadata_json` 已把 `{"turnMode":...}` 写入 user message 的 `attachment_ids_json` | 400 的产生点;元数据写入点 |
| 分类器测试 | `crates/agent/src/tests.rs:255-287` | 两处断言 Ambiguous 报错(`"继续"`、非空 graph 的 `"workflow"`) | 需按新契约更新 |
| Chat prompt 来源 | `crates/agent/src/prompt_stack.rs` | Chat 的运行时指令由 prompt_stack 渲染进 `ctx/instructions.md`(`mode_override` 管输出契约,`system_behavior` 管回复行为);skill 文件只在 `uses_graph_context()` 分支写入,Chat 返回 false,`selected_skill(Chat)` 文本不会进入 Chat 运行时 prompt | 澄清指引的落点 |
| messages 表 | `crates/store/src/workspace_records.rs` | 已有 `attachment_ids_json TEXT` 列存 turn 元数据 | 确认无需 migration |

## 设计方案

三个决策点,逐一回答:

### 1. Ambiguous 映射到 Chat 的位置:分类器内

在 `crates/agent/src/turn_mode.rs` 内处理,而不是在入口把 `Err(Ambiguous)` 翻译成 Chat。
理由:分类语义(含兜底)应有单一真实来源;若在入口翻译,分类器契约仍宣称
"Ambiguous 是需要澄清的错误",与系统实际行为矛盾,且未来其他调用方会重复实现兜底。

具体改动(不保留兼容层,直接删旧代码):

- 删除 `TurnRoutingError::Ambiguous` variant,`TurnRoutingError` 只剩 `EmptyMessage`。
- 新增分类结果类型,携带来源标记:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnModeSource {
    Keyword,
    AmbiguousFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnClassification {
    pub mode: TurnMode,
    pub source: TurnModeSource,
}
```

- `classify_turn_mode` 签名改为
  `fn classify_turn_mode(user_message: &str, graph: &WorkflowGraph) -> Result<TurnClassification, TurnRoutingError>`;
  原 `Err(Ambiguous)` 分支改为
  `Ok(TurnClassification { mode: TurnMode::Chat, source: TurnModeSource::AmbiguousFallback })`,
  所有关键词命中分支返回 `source: TurnModeSource::Keyword`。
- `crates/agent/src/lib.rs` 的 `pub use` 同步导出新类型。

### 2. 分类结果记入 message 元数据:复用 attachment_ids_json,无需 migration

`workbench_message.rs` 已通过 `turn_metadata_json` 把 `{"turnMode":"chat"}` 写入
user message 的 `attachment_ids_json`(GH21 既有机制)。本次扩展该 JSON:

- 关键词路径:保持 `{"turnMode":"chat"}` 字面量不变(不写 source key),
  既有测试的精确字符串断言不受影响。
- 兜底路径:写 `{"turnMode":"chat","turnModeSource":"ambiguous_fallback"}`。

字段名 `turnModeSource`(JSON key,camelCase,与既有 `turnMode` 一致)。
messages 表结构不变,**不需要 migration**。误分类率观察方式:
`SELECT COUNT(*) FROM messages WHERE attachment_ids_json LIKE '%ambiguous_fallback%'`。
`attachment_ids_json` 列名与用途已不符是 GH21 遗留问题,重命名不在本 issue 范围内。

### 3. Chat 澄清指引:落点是 prompt_stack 的 `system_behavior`,不是 skill 文件

`crates/agent/src/prompt_stack.rs` `system_behavior(TurnMode::Chat)` 文本追加一句:
当用户意图不明确时,主动问一句澄清(想创建 / 修改 / 运行还是调试 workflow)。

落点选择理由(回应 PR review 发现):`crates/agent/src/lib.rs` 中 skill 文件只在
`request.mode.uses_graph_context()` 分支写入,而 Chat 的 `uses_graph_context()` 返回
false,所以改 `selected_skill(AgentSkill::Chat)` 根本不会影响 Chat 的运行时 prompt。
Chat 的真实指令来自 prompt_stack 渲染进 `ctx/instructions.md` 的各层;其中
`mode_override` 管输出契约与权限边界,`system_behavior` 管回复行为约束,澄清指引
属于后者,故写死在 `system_behavior(TurnMode::Chat)`。

静态追加(而非按 source 条件注入)的理由不变:对普通闲聊无害,且避免把
`TurnModeSource` 传播进 `AgentSessionRequest` 扩大改动面。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 无关键词消息 → Chat,200 | `turn_mode.rs` 兜底分支 + `workbench_message.rs` | agent crate 单测:`"继续"` → Chat/AmbiguousFallback;server 单测:POST 无关键词消息返回 200 且 `turn_mode == Chat` |
| P2 空消息仍 400 | `TurnRoutingError::EmptyMessage` 保留 | agent crate 单测:空串/纯空白返回 `EmptyMessage` |
| P3 关键词路径行为不变 | 关键词分支仅包一层 `TurnClassification` | 既有分类测试仅改断言取 `.mode`,期望值不变;`cargo test --workspace` |
| P4 分类元数据可查询 | `turn_metadata_json` | server 单测:兜底消息 `attachment_ids_json` 含 `turnModeSource`;关键词消息保持 `{"turnMode":"chat"}` 原样 |
| 目标:兜底回复主动澄清意图 | `prompt_stack.rs` `system_behavior(TurnMode::Chat)` | agent crate 单测:对 Chat 模式 `build_prompt_stack(...).render()`(即写入 `ctx/instructions.md` 的文本)断言包含澄清指引 |

## 数据流

输入:POST `/api/workspaces/:id/messages` 的 `userMessage` + `graph`。
处理:`classify_turn_mode` → `TurnClassification` → 入口按 `mode` 分派(兜底走
`TurnMode::Chat` 既有分支,无新分支)→ `turn_metadata_json(classification)` 序列化
→ `store.create_message` 写入 messages 表 `attachment_ids_json`。
输出:`WorkspaceMessageResponse.turnMode`(兜底时为 `chat`)。
外部调用:无新增;agent runtime 调用走既有 Chat 路径。

## 备选方案

- **入口处翻译 `Err(Ambiguous)` → Chat**:diff 最小(只改 `workbench_message.rs`),
  但分类器契约与系统行为分裂,兜底语义散落两层,弃用。
- **给关键词路径也写 `turnModeSource:"keyword"`**:元数据更完整,但会改变既有
  元数据字面量、需要同步更新既有精确断言,对观察误分类率无增量价值,弃用。
- **按 source 条件注入澄清 prompt**:更精确,但需把 source 传进
  `AgentSessionRequest`,改动面扩大,静态指引已满足需求,弃用。
- **改 `selected_skill(AgentSkill::Chat)` skill 文件注入澄清指引**:skill 文件只在
  `uses_graph_context()` 分支写入 ctx,Chat 模式返回 false,该文本永远不会进入
  Chat 运行时 prompt,指引形同虚设(PR review 发现),弃用。

## 风险

- Security: 无新增输入面;兜底消息与普通 Chat 消息走完全相同的既有路径。
- Compatibility: 400 → 200 是错误路径收窄;前端无 ambiguous 特殊处理(已 grep 确认),无破坏。
- Performance: 仅多一个 Copy 结构体包装,无影响。
- Maintenance: `classify_turn_mode` 签名变更,调用方仅 `workbench_message.rs` 与 agent crate 测试,一次性改完。

## 测试计划

- [ ] Unit tests(agent crate):`"继续"` → `Chat` + `AmbiguousFallback`;非空 graph 的 `"workflow"` → 兜底 Chat;空 graph 的 `"workflow"` → `CreateWorkflow` + `Keyword`(行为不变);空串 → `EmptyMessage`;既有关键词用例断言 `.mode` 不变;Chat 模式 `build_prompt_stack(...).render()` 输出(即 `ctx/instructions.md` 文本)包含澄清指引。
- [ ] Integration tests(server crate):POST 无关键词消息 → 200、`turnMode: chat`、agent 回复落库、user message 元数据含 `turnModeSource: ambiguous_fallback`;既有 4 个 post_message 测试不改元数据断言即通过。
- [ ] Manual verification:本地起 server,发送"继续",确认收到 chat 回复且 sqlite 中元数据可查。

## 回滚方案

单 PR、无 schema 变更、无数据写入格式依赖(`turnModeSource` 是可选 key,旧代码用
`MessageMetadata` 反序列化时自动忽略未知字段)。`git revert` 该 PR 即完全回滚;
回滚后已写入的兜底元数据仅是多余 JSON key,不影响读取。
