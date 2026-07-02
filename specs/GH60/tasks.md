# Task Plan

## Linked Issue

GH-60

## Spec Packet

- Product: `specs/GH60/product.md`
- Tech: `specs/GH60/tech.md`

## 实现任务

- [ ] `SP60-T1` 分类器兜底改造 — Owner: single-agent。改 `crates/agent/src/turn_mode.rs`:删除 `TurnRoutingError::Ambiguous`,新增 `TurnClassification` / `TurnModeSource`,无关键词命中时返回 `Chat` + `AmbiguousFallback`;`crates/agent/src/lib.rs` 导出新类型;更新 `crates/agent/src/tests.rs` 中原 Ambiguous 断言。Done when: `"继续"` 分类为 Chat 兜底,空串仍 `EmptyMessage`,既有关键词用例期望值不变。Verify: `cargo test -p helixflow-agent`
- [ ] `SP60-T2` 入口接入与元数据 — Owner: single-agent。改 `crates/server/src/workbench_message.rs`:适配新签名,`turn_metadata_json` 在兜底时追加 `turnModeSource:"ambiguous_fallback"`(关键词路径字面量不变);新增无关键词消息 → 200 Chat 回复 + 元数据落库的测试。Done when: 无关键词消息不再 400,兜底元数据可查,既有 4 个 post_message 测试不改元数据断言即通过。Verify: `cargo test -p helixflow-server`
- [ ] `SP60-T3` Chat 澄清指引(prompt_stack)— Owner: single-agent。改 `crates/agent/src/prompt_stack.rs` `system_behavior(TurnMode::Chat)`:追加"用户意图不明时主动澄清(创建/修改/运行/调试 workflow)"一句指引;不改 `selected_skill(AgentSkill::Chat)`(skill 文件只在 `uses_graph_context()` 分支写入,Chat 不写)。新增/更新 agent crate 单测:对 Chat 模式 `build_prompt_stack(...).render()`(即写入 `ctx/instructions.md` 的文本)断言包含澄清指引。Done when: Chat 模式 prompt_stack 渲染输出含澄清指引且单测通过。Verify: `cargo test -p helixflow-agent`

## 并行拆分

不并行。本 issue 改动集中在 3 个文件且 T2 依赖 T1 的新类型,拆分收益为负,
单人按 T1 → T2 → T3 串行执行。

## 验证

- [ ] `SP60-T4` 全量回归 — Owner: single-agent。Done when: workspace 全量编译与测试通过,无既有行为回归。Verify: `cargo check --workspace && cargo test --workspace`

## Handoff Notes

- 关键词路径的元数据字面量(如 `{"turnMode":"chat"}`)是既有测试的精确断言,不要给关键词路径加 `turnModeSource` key。
- 不做 `attachment_ids_json` 列重命名、不加 migration、不引入 LLM 分类(非目标)。
- `crates/agent/src/tests.rs:262`(`"继续"` is_err)与 `:279`(非空 graph `"workflow"` is_err)是本次契约变更点,按新契约改为断言兜底 Chat,属预期测试演进,非削弱断言。
- 兜底路径复用 `TurnMode::Chat` 既有分派分支,入口 match 不新增分支。
- 澄清指引落点是 `prompt_stack.rs` 的 `system_behavior(TurnMode::Chat)`,不是 skill 文件:Chat 的 `uses_graph_context()` 返回 false,`crates/agent/src/lib.rs` 中 skill 文件不会写入 ctx,Chat 运行时指令全部来自 prompt_stack 渲染的 `ctx/instructions.md`(PR #71 review 发现)。
