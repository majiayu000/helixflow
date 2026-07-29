# Tech Spec

## Linked Issue

GH-154

## Product Spec

见 `specs/GH154/product.md`。

## Codebase Context

| Area | Current files | Current behavior | Required change |
| --- | --- | --- | --- |
| Store schema | `crates/store/migrations/0001_initial.sql`、`0004_run_retry_and_artifact_review.sql` | `runs`、`run_steps`、`artifacts`、`cost_ledger` 无远端 handle、output port 或 recovery lease | 新 migration 与 typed records；DB 成为 handle/output/lease 真相 |
| Restart cleanup | `crates/store/src/run_cleanup.rs` | `queued/estimating/running` 全部直接中断并 skip step | 分类 + lease claim + durable unrecoverable finalization |
| Gateway contract | `crates/gateway/src/lib.rs` | `Provider::invoke` 同时 submit/poll；`ProviderTaskHandle` 只有 provider/task id | 拆分 dispatch/resume，扩展 secret-free durable handle |
| Atlas | `crates/gateway/src/atlas.rs` | video submit 后 prediction id 只进 `in_flight`，同 future poll；cancel 明确 unsupported | prediction handle 返回 runner；支持 resume poll；不增加 cancel |
| fal | `crates/gateway/src/fal.rs` | status URL 只进 `in_flight`；同 future poll/result；可由 status URL cancel | 返回已验证 handle；resume poll/result；恢复前 revalidate；保留 cancel |
| Runtime registry | `crates/gateway/src/registry.rs`、`runtime_provider.rs` | 转发 invoke/active_handles/cancel | 按 handle provider 转发 dispatch/resume/cancel 与 capability |
| Executor | `crates/run/src/executor.rs` | outputs 和 DAG readiness 主要在内存；末尾遍历 step 写 actual cost | dispatch intent、CAS handle、幂等 step finalize、durable DAG reconstruction |
| Background/self-heal | `crates/run/src/background.rs`、`self_heal.rs`、`cost_gate.rs` | normal background future 驱动；failed 后同图重试 | 新 recovery coordinator；normal/recovered 共享 terminal/failure finalizer |
| Server startup | `crates/server/src/app_state.rs` | 构建 provider registry 前同步批量 interrupt | 先构建 AppState，再短扫描/认领并 spawn recovery |
| Events/UI | `crates/server/src/workspace_events.rs`、`workspace_state.rs`、`web/src/store-events.ts` | 支持 terminal/retry/remote-cancel 通知 | 新 recovery/risk event + durable system message；断线补拉和刷新后保留 notice |

## 1. 持久化模型

新增下一顺序 migration（实现时按 main 上最新编号命名），建立以下结构。

### 1.1 `run_provider_tasks`

```sql
CREATE TABLE run_provider_tasks (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  run_step_id TEXT NOT NULL REFERENCES run_steps(id) ON DELETE CASCADE,
  provider TEXT NOT NULL,
  dispatch_origin TEXT NOT NULL,
  recovery_scope_fingerprint TEXT NOT NULL,
  operation_key TEXT NOT NULL,
  state TEXT NOT NULL CHECK (
    state IN ('dispatching', 'active', 'result_ready', 'completed', 'cancelled', 'abandoned')
  ),
  dispatch_owner_id TEXT,
  dispatch_lease_expires_at TEXT,
  dispatch_deadline_at TEXT,
  provider_task_id TEXT,
  status_url TEXT,
  result_url TEXT,
  terminal_outcome TEXT,
  result_spool_path TEXT,
  result_fingerprint TEXT,
  last_error_code TEXT,
  recovery_deadline_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  ended_at TEXT,
  UNIQUE (run_step_id),
  UNIQUE (operation_key)
);
```

约束：

- `dispatching` 的 handle/result 字段为空，必须有 dispatch owner/lease；`active` 必须有
  非空 `provider_task_id`，provider 可按能力要求 status/result URL。
- successful `result_ready` 表示 provider terminal 与 actual cost 已持久化，必须有
  `terminal_outcome/result_fingerprint`，inline output 还需安全 spool；它不可被 cancel/
  abandon。artifact materialization 完成后才推进 `completed`。
- `completed/cancelled/abandoned` 是 task 终态，不可退回 active；`completed` 表示已知
  结果（同步成功、远端 terminal、确定未提交或明确拒绝），失败由 `last_error_code`
  区分并将 run/step 送入共同 failed finalizer。
- `operation_key` 由 run/step/provider/dispatch attempt 的稳定 identity 生成，不包含
  prompt、URL 或 secret。同一 `run_step_id` 终生至多一条 provider task；本 issue 的
  rejected/unknown dispatch 不在同一 step 上重提，resume/replay 只能推进原 row。
- `recovery_scope_fingerprint` 由 provider id、canonical API origin、可用的非秘密
  account/tenant identity 以及 credential identity 的单向摘要组成；不得保存 raw key。
  当前配置无法生成相同指纹时禁止 resume/cancel，即使 provider id 与 URL 仍相同。
- `dispatch_origin` 是提交时已验证的 canonical `scheme://host[:port]`；恢复时先验证其
  格式，再要求它与当前 provider canonical origin 完全相等。不得只从当前配置拼接旧
  task id 后直接请求。
- task id 和 URL 只在 store/gateway 内部使用，不序列化进普通 API/event。需要诊断时
  只返回 record id、provider 和 stable code。
- `active` CAS 同时写 provider policy 计算的绝对 `recovery_deadline_at`；后续进程只计算
  remaining duration，不得在 restart 时重置 deadline。
- dispatch owner 在绝对 `dispatch_deadline_at` 前周期续租；interrupt/failure settler
  看到有效 owner 只能请求停止并等待，不能直接 drop 正在提交的 future。
  owner 返回 Accepted/Completed 时仍有权完成 handle/result CAS，然后必须检测
  terminalization work item 并交 settler；只有 owner 丢失或 lease 过期时，startup 才能
  CAS dispatching → abandoned/outcome_unknown。owner 不得越过 absolute deadline 无限续租；
  到期仍无确定响应按 outcome unknown 收敛。

SQLite CHECK 无法完整表达状态字段组合时，Store 写 API 必须在 transaction 中校验并
返回 typed invariant error；禁止调用方直接拼 SQL 跨状态写入。

### 1.2 `run_step_outputs`

```sql
CREATE TABLE run_step_outputs (
  run_step_id TEXT NOT NULL REFERENCES run_steps(id) ON DELETE CASCADE,
  port TEXT NOT NULL,
  artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE RESTRICT,
  created_at TEXT NOT NULL,
  PRIMARY KEY (run_step_id, port),
  UNIQUE (artifact_id)
);
```

provider 与有输出的 builtin step 都写 port 映射。恢复 DAG 以 frozen `runs.plan_json`
结合该表重建 `OutputMap`；`run_step_artifacts` 的创建顺序和 artifact metadata 不能
替代 port identity。

### 1.3 Cost/event idempotency

为 `cost_ledger` 增加 nullable `operation_key`，建立
`UNIQUE(operation_key) WHERE operation_key IS NOT NULL`。新估算与实际费用分别使用
`estimate:{run_step_id}`、`actual:{run_step_id}`；旧记录保持 NULL。Store 提供
insert-or-read API，并核对同 key payload，不允许同 key 不同 amount/currency。

terminal event 不通过“先更新 run、后 append event”两步完成。每个 task/step
finalizer 在单 transaction 做自己的 CAS/event；terminalization work item 证明所有相关
task 已终态后，最后一个 transaction 才写 run terminal、run event、ended_at 与 work-item
completed。CAS 未命中时读取既有终态并返回 replay，不追加第二个事件。

### 1.4 `run_recovery_leases`

```sql
CREATE TABLE run_recovery_leases (
  run_id TEXT PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
  owner_id TEXT NOT NULL,
  lease_expires_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

claim、renew、release 都校验 owner。claim 只允许 active run 且 lease 缺失/已过期；
续租失败即 lease loss。owner 是进程启动生成的随机 opaque ID，不复用 hostname、
PID、credential。lease 不是分布式队列承诺，只防止旧 background future 或并发启动
重复恢复。

### 1.5 Execution intent 与 terminalization work item

新增 `run_execution_intents`（`run_id` UNIQUE），保存 frozen
`plan_fingerprint`、完整 `estimate_fingerprint`、cost-decision fingerprint、
`approval_kind = user_confirmation | auto_budget`、批准时间和
`approved | claimed | executing | terminal` 状态。

它必须与 `waiting_confirmation → queued/running` 或 auto-budget claim 同事务创建/推进。
只有 plan、每个 provider step 的 stable estimate ledger 和 decision 都完整且 fingerprint
一致，queued restart 才可复用该 intent；缺失或部分写入时不能把 queued 当成“已估价且
已批准”。

新增 `run_terminalization_work_items`（`run_id` UNIQUE），保存 desired run status、
source failed step、stable reason 与 `settling | ready | completed` 状态。任一 step
失败时，在同一 transaction 标记 source step/task terminal、创建 work item、撤销执行
owner 并禁止调度新 step；run 暂时保持 running，直到所有 sibling remote task 都持久化
为 `completed/cancelled/abandoned`。

同一表也承载 online interrupt：`desired_status=interrupted` 时，queued/estimating 且无
provider task 可在创建 work item 的同一 transaction 直接完成；running 若存在
dispatching/active/result_ready task，则写 `settling` work item、撤销 DAG 调度 owner，但 run 保持
running，使现有 active-run lease 可以恢复 settler；tasks 全部终态后才原子 interrupted。

同一 run 的 failure 与 interrupt 用单调 desired-status CAS 合并：

- 无 work item 时 insert；
- run 仍为 running 时，`failed/(settling|ready) + user interrupt` 原子升级为
  `interrupted/settling`，保留 source failure audit，但最终不创建 self-heal
  continuation；
- 已 `interrupted` 后到达的 step failure 只补 task diagnostic，不降级为 failed；
- work item completed 或 run 已 succeeded/failed/interrupted 后不再改写。

显式用户 interrupt 优先于尚未提交的 automatic failed terminal，避免用户停止后继续
self-heal。startup 先扫描所有未完成 work item；有 remote task 的 work item 关联 run
仍为 active，因此可按现有 lease 契约 claim。

新增 `run_failure_continuations`（`source_run_id` UNIQUE），保存 UNIQUE
`retry_key = retry:{source_run_id}:{next_attempt}`、
`pending | retry_created | exhausted | completed`、next attempt 与 UNIQUE 可空
`child_run_id`。失败 terminal transaction 与 continuation 同事务提交；retry child
insert/linkage 与 continuation 推进在同一 `BEGIN IMMEDIATE` transaction 完成，rollback
不会留下未关联 child。

不直接给历史 `runs(parent_run_id, attempt)` 建唯一索引，因为旧版本可能已存在重复数据，
会阻断 migration。新代码只通过 continuation `retry_key` 单赢家创建 child；migration
保留全部历史 runs，并有 fixture 证明包含旧重复 parent/attempt 的数据库仍可升级。
GH-153 后续在同一 continuation exhausted 分支接 Agent fix，不新建第二套 handoff。

新增 `artifact_publish_intents`，以 stable output operation key UNIQUE 保存 staging path、
canonical content-addressed path/hash、`staged | published | committed` 状态与 owner/时间。
file publish 前先 durable journal；artifact/output transaction 同时 mark committed。
启动 reconciliation 先重放仍可完成的 step finalizer；只有 intent 已过期、无 artifact row
引用、无 active task/terminalization 可继续引用时，才引用感知删除 orphan file 并完成
intent。删除失败显式 error 并保留 intent 重试，不得假设仓库已有 artifact GC。

provider terminal result 使用 durable spool 契约：每个 output 先清洗
`ArtifactPayload.meta`；inline text/bytes 写入 fsync 的 spool + journal。RemoteUrl 不把
signed URL 暴露到 event/API，能从 durable handle 重新获取时仅保存安全 fingerprint。
随后 transaction CAS `dispatching|active → result_ready`、insert-or-read
`actual:{run_step_id}` ledger，并记录 spool/fingerprint；从 `dispatching` 推进时还必须
匹配当前 `dispatch_owner_id`。artifact materializer 只消费 result_ready；下载/发布失败
保持 result_ready 并按独立、有界 materialization policy 重试。
到期将 step/run 送入 failed terminalization，但 task 仍作为 provider-completed，不调用
cancel/abandon。

## 2. Gateway 生命周期契约

把远端生命周期从单一 `Provider::invoke` 拆为可持久化边界；同步 provider 可直接返回
completed。

```rust
pub enum ProviderDispatch {
    Completed(ProviderResult),
    Accepted(DurableProviderTaskHandle),
}

pub struct DurableProviderTaskHandle {
    pub provider: String,
    pub dispatch_origin: String,
    pub recovery_scope_fingerprint: String,
    pub provider_task_id: String,
    pub status_url: Option<String>,
    pub result_url: Option<String>,
}

pub enum ProviderDispatchFailure {
    NotSubmitted(ProviderTerminalError),
    Rejected(ProviderTerminalError),
    OutcomeUnknown(ProviderUnknownError),
}

pub enum ProviderResume {
    Pending,
    Completed(ProviderResult),
    Failed(ProviderTerminalError),
}

#[async_trait]
pub trait Provider {
    async fn dispatch(
        &self,
        request: ProviderRequest,
        operation_key: &str,
    ) -> Result<ProviderDispatch, ProviderDispatchFailure>;
    async fn resume(
        &self,
        request: ProviderRequest,
        handle: &DurableProviderTaskHandle,
    ) -> ProviderResultValue<ProviderResume>;
    async fn cancel(
        &self,
        handle: DurableProviderTaskHandle,
    ) -> ProviderResultValue<()>;
}
```

`ProviderRequest` 由 frozen plan、durable upstream outputs 和 materialized input
重建。`operation_key` 只在 provider 明确支持幂等键时传给上游；Atlas/fal 当前不能
因为本地有 key 就假定 submit 可安全重试。

`NotSubmitted` 只允许用于 gateway 能证明没有发出请求的本地 validation/materialization
错误；收到明确的上游拒绝响应使用 `Rejected`；请求可能已被接收的 timeout、断线或无法
解析的成功响应一律是 `OutcomeUnknown`。前两类把 task 收敛为 `completed` +
`last_error_code`，把 step 标为 failed 并创建 terminalization work item；sibling 收敛后
才把 run 送入共同 failed finalizer。`OutcomeUnknown` 把当前 task 收敛为 `abandoned`、
写计费风险并创建/合并 `desired=interrupted` terminalization；全部 siblings 收敛后才
提交 run interrupted。三类均不得在 Atlas/fal 自动重提。

`active_handles` 和 Atlas/fal `in_flight` map 从 correctness path 删除。为兼容
同步 chat/image，可由 dispatch 返回 `Completed`；若进程在远端可能已接受请求但
dispatch 尚未返回时崩溃，DB 仍是 `dispatching`，按 unknown/abandoned 处理。

### 2.1 Atlas

- async video submit 返回 prediction id 后构造 `Accepted`，不在 gateway 内循环 poll。
- dispatch 时分别保存 canonical Atlas origin 与 account/credential identity 的非秘密
  scope 指纹；`resume` 先要求 persisted origin == current canonical origin 且 scope
  完全匹配，再以 persisted origin 构造/验证 prediction endpoint。禁止仅用新
  `ATLAS_API_BASE` 拼接旧 prediction id。
  completed/succeeded → `Completed`，failed → typed terminal failure，其余 → Pending。
- image/chat 当前同步 API 返回 `Completed`；dispatching crash window 不自动重发。
- `cancel` 继续返回 `CancelUnsupported`；测试必须断言没有生成或调用 cancel URL。

### 2.2 fal

- submit response 的 request id、status URL、response URL 均先通过
  `validate_callback_url`，然后返回 `Accepted`。
- dispatch 时保存 fal API origin/account/credential identity 的非秘密 scope 指纹；
  scope 不匹配时不得 poll、下载或 cancel。
- 对 provider 返回的原始 URL 先校验：存在 userinfo、fragment 或 credential-like query
  立即拒绝，不得通过剥离/规范化后继续接受；之后再保存 canonical origin/locator。
  恢复每次请求前再次对 persisted origin、当前 `FAL_API_BASE` 做 exact-origin 与 path
  校验，防止配置漂移或 DB tampering 造成 SSRF。
- `resume` poll status，COMPLETED 后读取 result URL 并返回 provider result。
- recovery 超时后的 cancel 复用现有 PUT cancel 语义；cancel URL 同样重新校验。

### 2.3 Mock

默认 mock 仍受双开关保护。测试 adapter 使用 frozen request +
`provider_task_id/operation_key` 确定性产生 Pending/Completed/Failed，不依赖旧进程
内存，用于覆盖 restart、lease 与 replay。mock artifact 必须继续带明确 mock 标识。

## 3. Dispatch 与 crash window

provider-backed step 执行顺序：

1. 完成 request materialization、provider scope 与静态 URL/schema preflight；失败时
   尚无网络请求，走普通 failed finalizer。
2. 将 step CAS 为 running，并以稳定 operation key insert-or-read
   `run_provider_tasks(dispatching)`，写 dispatch owner + expiry 并周期续租；若已有
   active/result_ready/terminal record，进入 resume/materialize/replay，不重新 submit。
3. 调用 `provider.dispatch`。
4. `Completed`：先 spool/fingerprint 安全 outputs，再原子写 task `result_ready` + actual
   ledger，之后才进入 artifact materializer；`NotSubmitted`/`Rejected` 原子写 task
   `completed`、step failed 与 terminalization work item；`OutcomeUnknown` 原子写 task
   `abandoned`、`desired=interrupted` work item 与计费风险，不直接 terminal run。
5. `Accepted`：严格校验 handle 和 scope，然后以
   `WHERE id=? AND state='dispatching' AND operation_key=? AND dispatch_owner_id=?` CAS 为
   active；即使已有 terminalization work item，live owner 也先保存 handle，再立即交
   settler。
6. CAS 成功后 poll/resume；CAS 未命中则停止并读取 durable truth，不让失去 lease 或
   被中断的 worker覆盖新状态。

关键窗口：

- crash 在步骤 2 后、provider 调用前：无法证明是否 submit，与步骤 3 已远端接受但
  本地未收到响应不可区分；只在 dispatch owner lease 过期后才能判 outcome unknown。
- crash 在步骤 3 成功后、步骤 5 commit 前：同样只留下 dispatching。
- 对两者统一 fail closed：不自动再次 dispatch，task → abandoned，创建/合并
  `desired=interrupted` work item，并写 `DISPATCH_OUTCOME_UNKNOWN` billing-risk；
  sibling tasks 收敛后才 run interrupted。
- gateway 返回的 typed `NotSubmitted`/`Rejected` 不是 crash window，不得误报
  abandoned；无法证明请求未提交时必须分类为 `OutcomeUnknown`。
- 只有未来某 provider 明确实现并测试 server-side idempotency lookup，才能在该
  provider 独立扩展 recovery；不得把本地 operation key 当作上游保证。

## 4. 幂等 step finalizer 与 DAG continuation

provider `Completed` 后先按 durable spool 契约把 task/cost 推进为 `result_ready`，再
下载或把 spool 发布为 content-addressed file；随后调用按来源携带 typed expected state
的共享 step finalizer：

| Source | Task row transition |
| --- | --- |
| synchronous dispatch `Completed` | `dispatching → result_ready`，先写 spool/cost |
| asynchronous `resume Completed` | `active → result_ready`，先写 spool/cost |
| asynchronous `resume Failed` | `active → completed`，同事务写 actual cost，无成功 output |
| artifact materialized | `result_ready → completed` |
| dispatch `NotSubmitted/Rejected` | `dispatching → completed`，无 output |
| local preflight failure | 无 task row，step `queued → failed` |
| builtin / cache hit | 无 task row，不执行 task CAS |

共享 store transaction：

1. 校验 recovery lease（normal execution 用显式 execution owner token）。
2. insert-or-read artifacts 与 `(step, port)` outputs；同 key 不同 payload/hash
   返回 invariant error。
3. result_ready replay 读取既有 actual ledger，不重复写；builtin/cache 按自身
   operation key insert-or-read cost。
4. 按上表 CAS 可选 task row；正常执行把 step `running → succeeded/failed`，preflight
   failure 明确使用 `queued → failed`。
5. append exactly-once `node.state`/recovery event。
6. 仅当该 step 之后整个 DAG 已 terminal，才在同一 transaction CAS run terminal；
   非最终 step 提交后 run 保持 running，由 coordinator 调度剩余 ready steps。

transaction rollback 不直接删除已完成的 content-addressed file；后续 replay 通过
`artifact_publish_intents` 使用同 hash/path。启动 reconciliation 按 journal 先重放可完成
finalizer，再按“无 DB 引用 + 无 active owner/work item + 已过期”三重条件清理 orphan；
不能先删可能被其他记录引用的文件。

现有 `try_cache_hit`、builtin step 与 provider completed 都必须调用该 finalizer；cache
hit 不得继续只复制 artifact 并直接标记 succeeded。必须覆盖“cache hit 已提交 output，
下游尚未启动即崩溃”的真实 reopen 测试。

任一 step 明确 failed 时不能立即把 run 置 failed。step finalizer 同事务创建
`run_terminalization_work_items(settling)` 并停止新调度；settler 对每个 sibling：

- 已 completed 的 task/step 保留合法 artifact/output/cost；
- `result_ready` 已是 provider terminal，不得 cancel/abandon；settler 可完成
  materialization，或在 desired terminal 下把 task completed + step skipped 并保留
  actual cost/spool 供审计；
- active 且支持 cancel 的 task 执行补取消并 CAS cancelled；
- active 且不支持或无法确认 cancel 的 task CAS abandoned，写各自 exactly-once 计费风险；
- 同时完成与 cancel 只由 task-state CAS 决定，完成胜者仍可持久化合法结果；
- local/builtin worker 收到 owner 撤销后停止，未完成 step 显式 skipped/interrupted。

只有所有 sibling task rows terminal 后，单 transaction 把 work item `ready → completed`、
run → failed、写 `run.failed` event，并调用共同 failure finalizer。启动扫描优先恢复
`settling` work item，即使 run 尚为 running；不依赖“只扫描 active run”偶然兜底。

恢复 run 从 `runs.plan_json` 载入同一 `ExecutionPlan`：

- 验证 plan version 与 run.version_id 一致；
- 读取所有 run_steps、run_step_outputs 和 artifacts；
- succeeded step 标记 finished，并把 durable outputs 注入 `OutputMap`；
- active provider task 先 resume；queued 且依赖已满足的 step 放回 ready queue；
- provider-backed running step 没有 active/terminal task 是 invariant violation，
  按 unrecoverable 处理；遗留部署前 active row也走该路径；
- 所有 step terminal 后使用共享 run finalizer；成功、失败、中断都以 CAS 防重。

## 5. Startup recovery coordinator

`AppState::open_in_data_dir` 调整为：

1. 打开唯一 `Store`，完成 migration 与 version file reconciliation。
2. 创建 provider registry、持久化 provider status。
3. 用该 `Store` clone 构造 `RunService` 和完整 `AppState`。
4. 严格解析 `HELIXFLOW_RUN_REQUEUE_ON_RESTART`：缺失/`0`/`false` 为 false，
   `1`/`true` 为 true，其他值返回 startup config error。
5. 短事务扫描 active rows并分类：
   - `waiting_confirmation`：不变；
   - `estimating`：interrupted；
   - `queued`：默认 interrupted；flag true、execution intent 与 plan/estimate/cost
     fingerprints 完全一致、per-step estimate ledger 完整且无 provider task 时，才
     lease claim 并走正常 claim/execute；任何部分状态显式 interrupted；
   - 有 `run_terminalization_work_items(settling)`：优先 claim settler，停止 DAG
     continuation；
   - `running` 且每个 running provider step 都有对应 active/result_ready/terminal task
     row、其余
     succeeded step 有 durable outputs、剩余 queued DAG 可由 plan 重建：lease claim；
   - `running` 且所有 step queued、execution intent 完整、无 provider task/output：
     lease claim 并从 ready queue 恢复；
   - `running` builtin step 仅当 capability 明确声明并测试 `restart_safe=true` 时可重放；
     否则创建/合并 `desired=interrupted` work item、撤销 DAG 并收敛全部 siblings 后
     terminal；
   - `dispatching` + valid dispatch owner：保持 row，claim/wait owner 返回，不得 abandon；
     owner lost/expired、provider-backed running 无 handle、非法 handle：把已知 task
     abandoned，创建/合并 `desired=interrupted` work item + durable risk event，再收敛
     siblings；
   - 其他未匹配 running 组合：创建/合并 interrupted work item + invariant event，禁止
     绕过 settler 或保留 stale running。
6. 对 claim 结果 spawn recovery future；HTTP server 启动不等待远端 poll。

recovery future 周期续租。暂时网络错误采用有界指数退避，但绝对期限只认 task
`recovery_deadline_at`；每次重启计算 `deadline - now`，到期立即尝试补取消/abandon，
不得重新获得完整窗口。deadline 的 cancel/abandon 结果也合并
`desired=interrupted` work item，收敛 siblings 后 terminal。有效 lease 被其他 owner
取代时，future 立即停止写入。

online interrupt 保留现有 `queued`、`estimating`、`running` 三种可中断状态，并使用
durable terminalization：

- queued/estimating 且无 provider task：transaction CAS 当前状态 → interrupted、终止
  本地 estimate/queue owner、skip 未完成 steps，并把 interrupt work item 直接 completed。
- running 且没有 dispatching/active/result_ready task：同样可在 transaction 直接 interrupted。
- running 存在 dispatching/active/result_ready task：transaction 只创建
  `run_terminalization_work_items(desired=interrupted, settling)`、撤销 DAG 调度 owner
  并发出 durable `run.interrupt_requested`；run 保持 running，API 表示请求已接受。
- dispatch 正在调用上游时若随后返回 Accepted，允许它仅为可追踪性完成
  `dispatching → active` CAS，但检测到 interrupt work item 后不得 poll/推进 DAG，立即
  交给 settler cancel。若进程在 handle CAS 前退出，startup 把 dispatching 标为
  abandoned + billing risk；valid owner 尚存时 settler 不允许抢先 abandon。
- active task 的 remote completion 与 cancel 竞争同一 task CAS；completion 胜者仍记录
  实际 cost 和脱敏后的 durable result，artifact 仅在 materialization 成功时发布，但两者
  都不得推进 DAG 或把 run 改成功。所有 task
  completed/cancelled/abandoned 后，settler 在同一 transaction 把 run → interrupted、
  写 terminal event 并把 work item completed。

因此进程在 interrupt 请求、handle 返回、cancel 调用或 terminal CAS 任一窗口退出，startup
都能从 work item 继续。进程内 cancellation token 只减少停机延迟，不是 durable truth。
非最终 step 在 interrupt work item 创建前完成时仍保留其合法 artifact/output/cost；
interrupt 后的 task finalizer 只有持有 matching work item 才可绕过 run-running guard，
且只能收敛 task/cost/safe artifact。最后 step 成功终态与 interrupt work-item claim
通过 run/work-item CAS 只允许一个路径生效。

workspace active-run 检查排除当前 recovery run；同 group sweep 保留既有例外。若 DB
存在互相冲突的多个 active run，按 `created_at,id` 确定顺序认领，不能同时恢复；
未认领者以稳定 conflict code 创建/合并 `desired=interrupted` work item，收敛其全部
sibling tasks 后再 terminal，不静默选择或遗留 paid task。

## 6. Unrecoverable 与用户可见事件

新增稳定事件：

| Event | Meaning |
| --- | --- |
| `run.recovery_started` | lease 已认领并开始恢复 |
| `run.recovery_succeeded` | 远端结果已幂等收敛并继续/完成 DAG |
| `run.recovery_failed` | provider 明确返回 failed，run 进入 failed |
| `run.recovery_cancelled` | 无法恢复但补取消成功，run interrupted |
| `run.recovery_abandoned` | 远端终态未知且无法确认取消，存在计费风险 |
| `run.requeued_after_restart` | 显式 flag 允许 queued run 重入 |

`run.recovery_abandoned` data 只允许：

```json
{
  "provider": "atlas",
  "code": "DISPATCH_OUTCOME_UNKNOWN",
  "message_id": "msg_recovery_task_...",
  "message": "远端任务状态无法确认，可能仍在运行并产生费用。"
}
```

禁止 provider task id、URL 和 raw error。Store transaction 同时写
`run_provider_tasks.abandoned`、run/step terminal state、run event，以及确定性
`message_id` 的 `role=system`/`kind=run_failed` workspace message。重复 finalizer
insert-or-read 同一 message，不能追加第二条。EventBus publish 发生在 commit 后；
publish 失败不影响 durable event/message。

Atlas/fal 的每一个 `ProviderResult.outputs[*].ArtifactPayload.meta` 在进入 artifact
finalizer 前必须移除 prediction id、request id、status/result URL 和 provider 原始
payload；workspace hydration、artifact API、event 与 Web 序列化测试对这些值做负向
断言。内部排障只能用 task record id 和稳定 reason code。

`web/src/store-events.ts` 将 recovery cancel/abandoned 映射为 system
`run_failed` notice，并优先复用 event 的 `message_id`；`workspace_state.rs` 通过既有
workspace messages 在全量 hydration 中返回同一 durable notice，因此刷新、断线后
出现新 run或最新 run 改变都不会丢失。snapshot merge 按 message id 去重。
`recovery_started/succeeded` 触发 state refetch。测试覆盖实时 WebSocket、seq gap
补拉、刷新/切 workspace，不将风险展示为成功或普通 chat。

## 7. #153 关系

#154 先实现统一 `finalize_run`/durable continuation 边界：

- normal executor 与 recovery coordinator 都在 run CAS 为 failed 的同一 transaction
  insert-or-read `run_failure_continuations`；coordinator 从该 durable intent 幂等创建或
  返回同一 retry child，不在 commit 后裸调用 `continue_self_heal_from_failed`；
- 本 issue 保留现有同图有界 retry 与 cost gate；
- #153 后续只在共同 finalizer 的“同图 retry 已耗尽”分支接入 Agent DebugWorkflow，
  不直接从 recovery worker 再建修图循环；
- recovered failed 的 `error_json`、parent/attempt 和 immutable plan 继续作为 #153
  审计输入。

因此 #153 依赖 #154；#154 不新增 Agent turn、version 或 fix attempt 字段。

## 8. Product-to-Test Mapping

| Invariant | Verification |
| --- | --- |
| dispatch-first + typed outcome + crash windows | store/run crash-point 与三类 dispatch failure 测试 |
| DB handle truth | reopened SQLite + interrupt/recovery integration |
| lease exclusivity | concurrent claim/renew/expiry tests |
| output/cost/terminal idempotency | repeated finalizer + restart tests |
| Atlas resume/no cancel | Atlas mock HTTP、account/scope 漂移 contract tests |
| fal resume/cancel/URL safety | fal mock HTTP + scope 漂移/malicious callback tests |
| mock deterministic recovery | run recovery tests |
| queued default/strict env | app_state config/startup tests |
| durable risk UI | workspace events + Web store event tests |
| cache/interrupt race/API secrecy | cache reopen、poll/interrupt/cancel race、artifact hydration 负向测试 |
| queued/running exhaustive classification | execution intent/partial estimate、all-queued、builtin safe/unsafe、无 handle |
| parallel failure settlement | sibling completed/cancelled/abandoned 后才 run.failed，settler restart |
| interrupt/continuation/deadline | 三种 interrupt status、dispatch race、retry 单例、deadline multi-restart |
| artifact journal/GC | publish-before-DB crash、replay、过期无引用 GC、引用保护与清理重试 |
| #153 handoff | recovered failure invokes existing self-heal finalizer once |

## 9. 风险与回滚

- **重复计费**：dispatching unknown 永不自动重发；所有 replay 先读 DB。
- **SSRF/secret**：URL 两次校验且不进入日志/event；credential 只从当前 env 注入请求。
- **长期 lease**：续租有上限；lease loss 终止写入，过期可重领。
- **磁盘 orphan**：artifact publish journal 先重放、后按引用/owner/expiry 三重 gate GC；
  删除失败保留 durable intent 并显式重试。
- **兼容性**：migration 前遗留 running row 无 handle，显式 interrupted/abandoned；
  waiting confirmation 不变。
- **回滚**：可关闭 recovery worker，但保留 schema、dispatch intent 与 DB-based online
  cancel；不得恢复内存 handle truth。queued requeue 始终可保持 false。

## 10. Verification

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test -p helixflow-store --locked run_recovery
cargo test -p helixflow-gateway --locked recovery
cargo test -p helixflow-run --locked recovery
cargo test -p helixflow-server --locked restart_recovery
cargo test --workspace --locked
cd web && npm ci && npx tsc --noEmit && npm test -- --run && npm run build
cd ..
git diff --check
```

当前 main 已在 `a1ee3eb` / `#157` 退役 repo-local SpecRail automation，
`checks/check_workflow.py` 不存在，因此它不是本规格的可执行 gate。规格 packet 结构
由人工审查，implementation PR 以 exact-head review、fresh GitHub Actions 和上述
构建/测试为准。
