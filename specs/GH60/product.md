# Product Spec

## Linked Issue

GH-60

## 用户问题

消息意图分类 `classify_turn_mode` 是纯关键词匹配。用户消息不含任何关键词时被判为
Ambiguous,接口直接返回 HTTP 400(`crates/server/src/workbench_message.rs:60`),
用户措辞稍有偏差就被硬拒,连一句回复都拿不到。这对一个对话式产品是不可接受的:
分类器的能力边界不应该变成用户的使用门槛。

## 目标

- 不含关键词的非空消息不再返回 400,兜底路由到 Chat 模式,由 agent 在回复中主动澄清意图。
- 分类结果(含"是否走了兜底")记入 message 元数据,便于后续观察误分类率。

## 非目标

- 用 LLM 做意图分类(等 A2 多轮 loop 落地后再评估)。
- 改进关键词表或分类准确率本身。
- 在前端 UI 上展示分类来源。

## Behavior Invariants

1. 任意非空自然语言消息 POST 到 workspace message 接口都能得到正常回复,不再出现
   400 ambiguous;不含关键词的消息按 Chat 处理并返回 `turnMode = "chat"`。
2. 空消息或纯空白消息仍返回 400(EmptyMessage 路径不变)。
3. 既有关键词路径行为不变:命中 run / debug / chat / modify / create / workflow 名词
   规则的消息,分类结果与改动前完全一致,现有测试不修改断言即可通过
   (仅原先断言 Ambiguous 报错的测试按新契约更新)。
4. 分类元数据可查询:每条 user message 的元数据都记录本次 `turnMode`;
   走兜底的消息额外带 `turnModeSource = "ambiguous_fallback"` 标记,
   可通过 messages 表直接统计兜底比例。

## 验收标准

- [ ] 不含关键词的消息(如"继续")得到 Chat 回复,HTTP 200,响应 `turnMode` 为 `chat`。
- [ ] 空消息仍返回 400。
- [ ] 既有关键词分类测试全部通过,行为无变化。
- [ ] 兜底消息在 messages 表元数据中带 `ambiguous_fallback` 标记,关键词路径元数据格式不变。
- [ ] `cargo test --workspace` 通过。

## 边界情况

- 纯空白消息(空格/换行):仍走 EmptyMessage → 400。
- 非空 graph 下的裸词"workflow":原先 Ambiguous → 400,现在兜底为 Chat
  (空 graph 下仍按现有规则判为 CreateWorkflow,行为不变)。
- 兜底消息的 agent 回复本身失败(agent runtime 错误):按现有错误路径返回,不属于本改动范围。

## 发布说明

- 无数据迁移:元数据复用 messages 表现有 `attachment_ids_json` 字段,仅新增可选 JSON key。
- API 兼容:400 ambiguous 是错误路径收窄为正常路径,前端无针对 ambiguous 的特殊处理,无需同步改动。
