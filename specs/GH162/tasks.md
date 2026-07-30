# Task Plan

## Linked Issue

GH-162

## Spec Packet

- Product: `specs/GH162/product.md`
- Tech: `specs/GH162/tech.md`

## 实现任务

| ID | Owner | Dependencies | Task | Done When | Verify |
| --- | --- | --- | --- | --- | --- |
| `SP162-T0` | store | — | 新增 0010 observation schema、typed enums/records、message+started 和 terminal CAS transaction、startup interrupted finalizer。 | user message 与 started 原子；terminal exactly once；terminal rows 不可改写；workspace cascade。 | `cargo test -p helixflow-store agent_contract_observation` |
| `SP162-T1` | server | `T0` | 严格解析可空 release/build attribution；graph-edit turn 在 Agent 前创建 observation，并在 intent/legacy success、clarify、Agent/runtime/compile error 分支终态化。 | 业务成功与 observation 同事务；错误请求仍有 durable observation；写入失败显式返回。 | `cargo test -p helixflow-server agent_contract_observation` |
| `SP162-T2` | server/store | `T0`,`T1` | 实现 authenticated evidence aggregate 与 time/release/build filters；抽取 #144 无副作用 current-graph evaluator并生成 migration aggregate。 | 返回 raw counts/rate/reason maps/rollback events/limitations；零样本和 stale assessment 不误报完成。 | `cargo test -p helixflow-server agent_contract_evidence` |
| `SP162-T3` | security/review | `T0`–`T2` | 增加恶意输入、restart、fault/replay、跨 workspace、unattributed、current graph missing 测试；执行全量 fresh verification 与 exact-head review。 | DB/API 无 raw prompt/error/secret/path；无 actionable review finding；CI green。 | `cargo fmt --all -- --check && cargo check --workspace --locked && cargo test --workspace --locked && git diff --check` |

## 顺序与提交边界

1. T0 先建立 durable truth；server 不得先用 message 文本或 EventBus 拼临时统计。
2. T1 对 graph-edit turn 做完整 terminal branch mapping；不得只覆盖 happy path。
3. T2 的 migration aggregate 只读 actual current graph，不把 `semantics_json` 或旧
   assessment 当 canonical completion。
4. T3 在实现 head 上做 fresh review。spec PR 与 implementation PR 分开；implementation
   最终使用 `Closes #162`，#146 只使用 `Refs #146`。

## Verification

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked
git diff --check
```

本 tranche 不改 Web，不需要执行 Web test/build。若实现中增加或修改前端，再补跑：

```sh
cd web
npm ci
npm test
npm run build
```

## Handoff Notes

- release/build 缺失保持 NULL；不要从 tag、Cargo version 或日期猜测。
- Agent logs/EventBus 只服务实时 UI，不是 evidence source。
- terminal error 必须先持久化；禁止 warning + continue。
- proposal/version/message success 与 observation completion 必须同 transaction。
- `rollbackEvents` 只统计真实 legacy turns；flag off 启动本身不算演练。
- evidence API 返回事实，不返回 `passed` 或自创阈值。
- migration complete 需要至少一个 current version 且全部 actual graph 为 canonical v2。
- 本 issue 不删除 legacy code；它只让 #146 的删除 gate 可被真实数据证明。
