# Tech Spec

## Linked Issue

GH-146

## Product Spec

见 `specs/GH146/product.md`。

## Current Code Map

| Area | Current path | Legacy responsibility to remove |
| --- | --- | --- |
| Switch | `crates/server/src/workbench_message_intent.rs` | 解析 `HELIXFLOW_AGENT_INTENT_CONTRACT` |
| State | `crates/server/src/app_state.rs` | `use_intent_contract` snapshot 与 test defaults |
| Request | `crates/agent/src/lib.rs` | `AgentSessionRequest.use_intent_contract` |
| Output routing | `crates/agent/src/turn_mode.rs`, `prompt_stack.rs` | boolean 选择 `IntentJson` / `ProposalJson` |
| Runtime read | `crates/agent/src/contract.rs`, `service.rs` | `proposal.json` decode、validate、retry |
| Server graph edit | `crates/server/src/workbench_message.rs` | legacy `propose_graph_change` branch |
| Run fix | `crates/server/src/workbench_message_run_fix.rs` | legacy fix proposal branch |
| Trait | `crates/server/src/app_state.rs` | `WorkbenchAgent::propose_graph_change` |
| Evidence | `crates/server/src/agent_contract_observation.rs` | runtime mode selection；历史聚合仍需保留 |
| Docs | `README.md`, `docs/PROMPT_DESIGN.md`, `docs/AGENT_RUNTIME_PROVIDER_SPEC.md` | 当前行为仍声明 `proposal.json` |

历史 specs 是审计记录，不回写成“从未存在 legacy”。当前 docs 必须更新。

## Pre-implementation Gate

实现分支创建前，维护者 comment 必须提供：

```json
{
  "releaseId": "v0.2.0",
  "buildRevision": "65504bf53f2cabff408518748fd471a709b1c369",
  "deploymentId": 5687357563,
  "window": {
    "since": "2026-07-31T06:34:00Z",
    "until": "<exclusive UTC>",
    "elapsedHours": 168,
    "nonEmptyUtcDays": ["...", "...", "..."]
  },
  "intent": {
    "total": 20,
    "success": 18,
    "clarify": 1,
    "error": 1,
    "successRate": 0.9,
    "clarifyReasons": {},
    "errorReasons": {}
  },
  "attribution": {"unattributed": 0, "inFlight": 0},
  "migration": {
    "totalCurrentVersions": 1,
    "alreadyMigrated": 1,
    "migratable": 0,
    "needsResolution": 0,
    "failed": 0,
    "missingOrCurrentless": 0,
    "complete": true
  },
  "rollback": {
    "legacySuccess": 1,
    "restoredIntentSuccess": true,
    "healthReady": true
  }
}
```

示例数字不构成实际证据。gate verifier 必须从 issue comment 的 raw API responses 重算，
不能信任手写 `passed`。

## Agent Contract Convergence

### Request and output routing

- 删除 `AgentSessionRequest.use_intent_contract`。
- graph-edit `TurnMode` 的 output contract 固定为 `IntentJson`；Chat/RunRequest 保持现状。
- `output_contract_with(bool)` 收敛为不接收 runtime switch 的确定性 API。
- graph-edit prompt 只声明 `out/intent.json`，并明确拒绝低层 node/edge/binding/connector。

### Runtime

- 删除生产 `AgentService::propose_graph_change`、proposal retry loop、
  `read_validated_proposal` 和对 `out/proposal.json` 的 graph-edit读取。
- 如果 `ValidatedAgentProposal` 仍被 server compiler/application transaction 用作内部
  carrier，应移动到更合适的 crate或改名，不能让 Agent runtime public API 暗示仍接受
  legacy output。
- 保留 chat reply 和 run request 的独立 output contracts。
- 删除 proposal-only validation feedback、retry counters 和 status events；Intent retry
  行为不弱化。

## Server Convergence

### Normal graph-edit turn

`post_workspace_message` 对 Create/Modify/Debug：

1. 原子写 user message + `intent/started` observation；
2. 调用 `propose_intent`；
3. 持久化 Agent logs；
4. compile；
5. success/clarify/error 走 GH162 已有 exactly-once terminal transaction。

删除：

- env switch parser；
- `AppState.use_intent_contract`；
- `contract_mode(bool)`；
- legacy Agent call；
- `LEGACY_PROPOSAL_APPLIED` 新写入分支。

`AgentContractMode::Legacy` 或等价 read representation 可为历史 rows 保留；任何新 turn
不得构造它。evidence API 继续汇总历史 legacy rollback event。

### Run-fix

`prepare_fix_proposal` 总是：

1. `propose_intent`；
2. compile against exact failed source graph；
3. preview as `ProposalKind::Fix`；
4. enforce existing failed-node dependency scope；
5. persist new version and child/cost decision。

删除 legacy fix branch，但不改变 retry budget、provider/catalog fingerprint、cost gate 或
quiescent cancellation。

### Tests and traits

- `WorkbenchAgent` 删除 `propose_graph_change`。
- 测试 doubles 只实现 chat + intent；不为编译通过保留 no-op legacy method。
- 现有 legacy-only tests 改为：
  - Intent success/clarify/error；
  - unexpected `proposal.json` 被拒绝；
  - env var 即使设置为 0 也不能改变 contract（优先完全不读取该变量）；
  - run-fix intent path；
  - historical legacy evidence still readable。

## Data Compatibility

- 不修改或 drop `agent_contract_observations`。
- 不 UPDATE 历史 `contract_mode='legacy'`。
- 新 observation 固定 `intent`。
- v0.2.0 和删除版必须都能打开包含 migration 0010 与历史 legacy rows 的同一数据库副本。
- 若需要 schema migration，只允许 additive；任何 destructive migration 需要新 spec。

## Unsupported Inputs

Catalog enum 是 source of truth。以 `aspect_ratio` 为例：

- `1:1 / 9:16 / 16:9` 可编译；
- `4:3` 返回稳定 typed error；
- 不自动近似、不改写为默认、不 fallback legacy。

删除 PR 增加回归测试，证明 contract convergence 不会弱化该 fail-closed边界。

## Binary Rollback Drill

在删除 PR exact head 通过测试后：

1. 复制 canary data dir 到临时、权限受限目录；
2. 用删除版 binary 打开副本，执行一个 Intent modify；
3. 正常停止删除版；
4. 用 `v0.2.0@65504bf...` binary 打开同一副本；
5. 查询 health/ready、workspace current/version history 和 evidence；
6. 执行一个默认 Intent modify，不设置 legacy flag；
7. 正常停止 v0.2.0，再用删除版打开副本并复核 current；
8. 删除临时副本；不得对唯一 canary database做破坏性演练。

日志和 gate comment 只保留 stable IDs/counts，不上传 prompt、graph、secret、绝对路径或
数据库文件。

## Verification

```sh
rg -n "HELIXFLOW_AGENT_INTENT_CONTRACT|use_intent_contract|propose_graph_change|proposal\\.json" \
  crates README.md docs
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked
cd web
npm ci
npm test
npm run build
git diff --check
```

`rg` 允许命中明确描述“旧输出已不再接受”的安全文档/负向测试；每个生产命中必须在 PR
review 中解释，否则 gate 失败。

## Rollout

删除版先部署到 `local-canary` 新 GitHub Deployment，使用 v0.2.0 canary database 副本
完成 binary rollback drill。drill 与 exact-head review green 后才合并/发布。出现回归时
回滚部署到 v0.2.0，不在 forward head 恢复 env switch。

