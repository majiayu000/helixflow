# Tech Spec

## Linked Issue

GH-162

## Product Spec

见 `specs/GH162/product.md`。

## Current Codebase Context

| Area | Current files | Required change |
| --- | --- | --- |
| Agent lifecycle | `crates/agent/src/service.rs` | status 进入内存 EventBus/Agent logs；不作为 durable truth |
| Turn orchestration | `crates/server/src/workbench_message.rs`, `workbench_message_intent.rs` | 在 Agent 调用前创建 observation，并在每个 terminal branch 原子终态化 |
| Proposal apply | `crates/server/src/workbench_message_proposals.rs`, `crates/store/src/proposal_records.rs` | proposal/version/message transaction 同时提交 success observation |
| Messages | `crates/store/src/workspace_records.rs` | user message 与 started observation 原子创建；clarify message 与 terminal observation 原子创建 |
| Migration | `crates/server/src/version_migration_routes.rs`, `version_semantics.rs` | 抽取无副作用 evaluator，用实际 current graph 生成 fleet aggregate |
| Persistence | `crates/store/migrations/0009_run_agent_fix.sql`, `crates/store/src/lib.rs` | 新增 0010 observation schema、typed records 与聚合查询 |
| API | `crates/server/src/ops_routes.rs`, `main.rs` | 增加 authenticated read-only evidence endpoint |

## Configuration

AppState 启动时读取两个可选值：

- `HELIXFLOW_RELEASE_ID`：可空；非空只允许 ASCII alphanumeric、`.`、`-`、`_`，
  长度 1–64。
- `HELIXFLOW_BUILD_REVISION`：可空；同一字符集，长度 1–128。

非法 UTF-8、空白值、超长或非法字符使 server 初始化失败。缺失保持 `None`，不从 git、
Cargo version 或当前日期猜测正式 release。AppState 将同一 attribution snapshot 复制到
每个新 observation。

## Persistence

### Schema

新增 `0010_agent_contract_observations.sql`：

```sql
CREATE TABLE agent_contract_observations (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  user_message_id TEXT NOT NULL,
  session_id TEXT,
  contract_mode TEXT NOT NULL CHECK (contract_mode IN ('intent', 'legacy')),
  outcome TEXT NOT NULL CHECK (
    outcome IN ('started', 'success', 'clarify', 'error')
  ),
  reason_code TEXT,
  release_id TEXT,
  build_revision TEXT,
  started_at TEXT NOT NULL,
  completed_at TEXT,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
  FOREIGN KEY (user_message_id) REFERENCES messages(id) ON DELETE CASCADE,
  UNIQUE (user_message_id),
  CHECK (
    (outcome = 'started' AND reason_code IS NULL AND completed_at IS NULL)
    OR
    (outcome <> 'started' AND reason_code IS NOT NULL AND completed_at IS NOT NULL)
  )
);

CREATE INDEX idx_agent_contract_observations_window
  ON agent_contract_observations(started_at, release_id, contract_mode, outcome);
```

不保存 raw error 或 detail JSON。terminal row append-only 由 store API 保证；数据库
CHECK 保证 started/terminal 字段完整。

### Store operations

新增 typed enums `AgentContractMode`、`AgentContractOutcome` 和稳定 reason code wrapper；
API boundary 使用 snake_case，HTTP DTO 由 serde 输出 camelCase。

1. `create_graph_edit_message_with_observation`：
   `BEGIN IMMEDIATE`，插入 user `messages` row 和唯一 `started` observation，commit 后返回
   两个 records。
2. `finalize_agent_contract_error`：
   条件 UPDATE `outcome='started'`；若已 terminal 且完全相同则 replay，若 outcome/code
   不同返回 conflict。
3. `create_clarification_and_finalize_observation`：
   同一 transaction 插入 clarify message并执行 terminal CAS。
4. proposal apply primitive 增加 observation completion input，在当前
   proposal/version/current/message transaction 内执行 success CAS。legacy 与 intent
   只使用不同 stable success code。
5. `finalize_interrupted_agent_contract_observations`：
   server 启动接受请求前，将全部 `started` 更新为
   `error/PROCESS_INTERRUPTED`，重复启动更新 0 rows。

所有 terminal operation 必须验证 observation 的 workspace、message 和 contract mode；
不得让其他 workspace 的 ID 被终态化。

## Server flow

### Graph-edit turn

1. 完成 turn classification、workspace/version读取。
2. 从 AppState snapshot 得到 `contract_mode`。
3. 原子持久化 user message + started observation。
4. 构造 Agent request并调用对应 contract。
5. 对 terminal branch：
   - Intent compiled/applied：
     proposal/version/message transaction `→ success/INTENT_COMPILED`。
   - Legacy proposal applied：
     同一 transaction `→ success/LEGACY_PROPOSAL_APPLIED`。
   - Compiler clarify：
     clarify message transaction `→ clarify/<compiler reason code>`。
   - Agent/runtime/output error：
     `→ error/<typed agent reason code>` 后返回原 API error。
   - compile/preview/apply precondition error：
     若业务 transaction 未开始/未提交，先 `→ error/<typed server code>`；
     proposal transaction 内 store/CAS error 回滚 success observation，随后单独完成 error
     observation；若该写入也失败，返回 store error。

Agent logs 继续用于 UI，但不作为 evidence truth。所有 error mapping 是穷举 match；
禁止 `format!("{error}")` 进入 reason code。

### Crash recovery

`AppState::open_in_data_dir` 在 migration 完成且任何 background worker/HTTP listener 启动前
调用 `finalize_interrupted_agent_contract_observations`。因为单实例 local-first server
启动时不存在本进程的有效请求，所有遗留 started 都属于前一进程。该动作只写稳定 code，
不恢复或重放 Agent turn。

## Evidence API

`GET /api/ops/agent-contract-evidence`

Query：

- `since`：RFC3339 UTC，必填；
- `until`：RFC3339 UTC，必填，exclusive；
- `releaseId`：可选，精确匹配；
- `buildRevision`：可选，精确匹配。

响应：

```json
{
  "window": {"since": "...", "until": "..."},
  "filter": {"releaseId": "v0.2.0", "buildRevision": "abc123"},
  "intent": {
    "total": 100,
    "success": 92,
    "clarify": 5,
    "error": 3,
    "successRate": 0.92,
    "clarifyReasons": {"MISSING_INPUT": 5},
    "errorReasons": {"AGENT_RUNTIME_ERROR": 2, "INTENT_COMPILE_ERROR": 1}
  },
  "legacy": {
    "total": 1,
    "success": 1,
    "clarify": 0,
    "error": 0,
    "rollbackEvents": 1
  },
  "attribution": {"unattributed": 0, "inFlight": 0},
  "migration": {
    "totalCurrentVersions": 4,
    "alreadyMigrated": 4,
    "migratable": 0,
    "needsResolution": 0,
    "failed": 0,
    "missingOrCurrentless": 0,
    "approvedIsolation": 0,
    "complete": true
  },
  "limitations": []
}
```

`successRate` 在 total=0 时为 `null`。reason maps 只包含 stable code。API 不返回
`passed`；阈值和最终判断留给 #146 gate packet。

Store 聚合仅访问 observation table。migration 聚合在 server 层列出 workspaces/current
versions，读取并 hash 验证 graph file，再调用从 #144 dry-run 路径抽取的纯 evaluator。
它不写 assessment、不 apply migration、不改变 current。读取失败计入 `failed` 或
`missingOrCurrentless` 并返回稳定 reason count，不能让整个列表静默变空。

## Security and privacy

- observation schema 无 text/JSON/blob 字段。
- release/build parser 拒绝 `/`、`\`、空白、URL 和控制字符。
- reason code 只来自 declared enum/upstream compiler stable code allowlist。
- aggregate endpoint 不返回 workspace/message/session/version identity。
- auth 继续由全局 middleware 保护；不增加 public metrics listener。
- tests 向 prompt/error/graph 注入 token、URL、绝对路径，断言 DB/API 均不包含原值。

## Product-to-test mapping

| Invariant | Verification |
| --- | --- |
| message + started atomicity | store transaction rollback/fault test |
| exactly-once terminal outcome | replay and conflicting completion store tests |
| success transaction coupling | proposal/version/message/observation integration tests |
| error persistence | Agent runtime/output and compile error API tests |
| restart finalization | reopen AppState with started fixture |
| release/build attribution | strict parser + null/filter tests |
| evidence aggregate | success/clarify/error/legacy/time/release API matrix |
| migration current truth | current graph replacement/stale assessment/missing file tests |
| no sensitive data | malicious input DB and response scan |

## Rollout and rollback

Migration 0010 is additive. On rollout, set release/build values explicitly and query a bounded
window after real usage. On application rollback, older binaries ignore the new table; records stay
available for later audit. Do not drop the table during #146 legacy deletion. If evidence recording
causes errors, fix the recording path; do not add warning-only fallback.
