use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use helixflow_gateway::{
    CostEstimate, DurableProviderTask, MockProvider, Provider, ProviderCatalog, ProviderError,
    ProviderHealth, ProviderRecoveryCapabilities, ProviderRequest, ProviderResult,
    ProviderResultValue, ProviderResume, ProviderTaskHandle,
};
use helixflow_graph::{ExecutionPlan, ExecutionStep, GraphEdge, GraphNode, WorkflowGraph};
use helixflow_store::{
    NewCostLedger, NewProviderTask, NewRun, NewRunStep, NewVersion, ProviderTaskHandleUpdate,
    RunRecord, Store, VersionSource,
};
use serde_json::json;
use tokio::sync::Notify;

use super::{AgentRunRequest, RunService, SweepPlan, SweepVariant};

const INVALID_RETRY_ENV_CHILD: &str = "HELIXFLOW_TEST_INVALID_RETRY_ENV_CHILD";

#[tokio::test]
async fn restart_resumes_active_provider_task_and_durable_dag() {
    let (store, _dir) = open_self_heal_store().await;
    let (workspace_id, version_id) = self_heal_workspace_version(&store).await;
    let plan = ExecutionPlan {
        schema_version: 1,
        version_id: version_id.clone(),
        catalog_revision: None,
        steps: vec![ExecutionStep {
            node_id: "writer-recovery".to_owned(),
            node_type: "llm.prompt_writer".to_owned(),
            provider: Some("mock".to_owned()),
            capability: Some("prompt_writer".to_owned()),
            inputs: BTreeMap::new(),
            params: json!({ "style": "recovered" }),
            resolved: None,
        }],
    };
    let plan_json = serde_json::to_string(&plan).expect("serialize recovery plan");
    let run = store
        .create_run(NewRun {
            workspace_id: &workspace_id,
            version_id: &version_id,
            group_id: None,
            label: "Recovery",
            trigger: "manual",
            plan_json: Some(&plan_json),
            estimate_json: None,
            status: "running",
        })
        .await
        .expect("create recovery run");
    let step = store
        .create_run_step(NewRunStep {
            run_id: &run.id,
            node_id: "writer-recovery",
            node_type: "llm.prompt_writer",
            provider: Some("mock"),
            state: "running",
        })
        .await
        .expect("create recovery step");
    let operation_key = format!("dispatch:{}:{}:mock", run.id, step.id);
    let task = store
        .insert_or_read_provider_task(NewProviderTask {
            run_id: &run.id,
            run_step_id: &step.id,
            provider: "mock",
            dispatch_origin: "provider://mock",
            recovery_scope_fingerprint: "",
            operation_key: &operation_key,
            dispatch_owner_id: "dead-process",
            dispatch_lease_seconds: 30,
            dispatch_deadline_seconds: 30,
        })
        .await
        .expect("persist dispatch intent");
    store
        .activate_provider_task(ProviderTaskHandleUpdate {
            task_id: &task.id,
            dispatch_owner_id: "dead-process",
            provider_task_id: "opaque-provider-task",
            status_url: None,
            result_url: None,
            recovery_deadline_seconds: 300,
        })
        .await
        .expect("activate provider task")
        .expect("active provider task");

    RunService::with_provider(store.clone(), RecoveringMockProvider)
        .recover_after_restart()
        .await
        .expect("schedule recovery");
    wait_for_self_heal_status(&store, &run.id, "succeeded").await;

    assert_eq!(
        store
            .provider_task(&task.id)
            .await
            .expect("provider task")
            .state,
        "completed"
    );
    assert_eq!(
        store
            .run_step_outputs(&run.id)
            .await
            .expect("durable outputs")
            .len(),
        1
    );
    let actual = store
        .cost_ledger_for_run(&run.id)
        .await
        .expect("actual ledger")
        .into_iter()
        .filter(|entry| !entry.estimated)
        .count();
    assert_eq!(actual, 1);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let events = store.run_events(&run.id).await.expect("recovery events");
            if events
                .iter()
                .any(|event| event.ev == "run.recovery_started")
                && events
                    .iter()
                    .any(|event| event.ev == "run.recovery_succeeded")
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("recovery events timeout");
}

#[tokio::test]
async fn restart_queued_requires_complete_intent_estimate_and_atomic_claim() {
    let (store, _dir) = open_self_heal_store().await;
    let (workspace_id, version_id) = self_heal_workspace_version(&store).await;
    let plan = ExecutionPlan {
        schema_version: 1,
        version_id: version_id.clone(),
        catalog_revision: None,
        steps: vec![ExecutionStep {
            node_id: "writer-queued".to_owned(),
            node_type: "llm.prompt_writer".to_owned(),
            provider: Some("mock".to_owned()),
            capability: Some("prompt_writer".to_owned()),
            inputs: BTreeMap::new(),
            params: json!({}),
            resolved: None,
        }],
    };
    let plan_json = serde_json::to_string(&plan).expect("serialize plan");
    let run = store
        .create_run(NewRun {
            workspace_id: &workspace_id,
            version_id: &version_id,
            group_id: None,
            label: "Queued recovery",
            trigger: "manual",
            plan_json: Some(&plan_json),
            estimate_json: Some(r#"{"amount":0.1}"#),
            status: "queued",
        })
        .await
        .expect("create queued run");
    let service = RunService::new(store.clone());
    let steps = service
        .ensure_run_steps(&run.id, &plan)
        .await
        .expect("persist run steps");
    service
        .ensure_execution_intent(&run.id, &plan, run.estimate_json.as_deref())
        .await
        .expect("persist intent");
    assert!(
        !service
            .claim_restart_queued_run(&run)
            .await
            .expect("partial estimate rejects")
    );
    store
        .create_cost_ledger(NewCostLedger {
            workspace_id: &workspace_id,
            run_id: Some(&run.id),
            run_step_id: Some(&steps[0].id),
            provider: "mock",
            amount: 0.1,
            currency: "USD",
            estimated: true,
        })
        .await
        .expect("create estimate");
    assert!(
        service
            .claim_restart_queued_run(&run)
            .await
            .expect("complete intent claims")
    );
    assert_eq!(
        store.run(&run.id).await.expect("claimed run").status,
        "running"
    );
}

#[tokio::test]
async fn restart_finishes_pending_terminalization_before_recovery_scan() {
    let (store, _dir) = open_self_heal_store().await;
    let (workspace_id, version_id) = self_heal_workspace_version(&store).await;
    let plan = ExecutionPlan {
        schema_version: 1,
        version_id: version_id.clone(),
        catalog_revision: None,
        steps: vec![ExecutionStep {
            node_id: "writer-terminal".to_owned(),
            node_type: "llm.prompt_writer".to_owned(),
            provider: Some("mock".to_owned()),
            capability: Some("prompt_writer".to_owned()),
            inputs: BTreeMap::new(),
            params: json!({}),
            resolved: None,
        }],
    };
    let plan_json = serde_json::to_string(&plan).expect("serialize plan");
    let run = store
        .create_run(NewRun {
            workspace_id: &workspace_id,
            version_id: &version_id,
            group_id: None,
            label: "Terminal recovery ordering",
            trigger: "manual",
            plan_json: Some(&plan_json),
            estimate_json: Some(r#"{"amount":0.1}"#),
            status: "running",
        })
        .await
        .expect("create run");
    let step = store
        .create_run_step(NewRunStep {
            run_id: &run.id,
            node_id: "writer-terminal",
            node_type: "llm.prompt_writer",
            provider: Some("mock"),
            state: "running",
        })
        .await
        .expect("create step");
    let task = store
        .insert_or_read_provider_task(NewProviderTask {
            run_id: &run.id,
            run_step_id: &step.id,
            provider: "mock",
            dispatch_origin: "provider://mock",
            recovery_scope_fingerprint: "",
            operation_key: "dispatch:terminal-ordering",
            dispatch_owner_id: "dead-process",
            dispatch_lease_seconds: 30,
            dispatch_deadline_seconds: 30,
        })
        .await
        .expect("create task");
    store
        .activate_provider_task(ProviderTaskHandleUpdate {
            task_id: &task.id,
            dispatch_owner_id: "dead-process",
            provider_task_id: "remote-terminal-ordering",
            status_url: None,
            result_url: None,
            recovery_deadline_seconds: 300,
        })
        .await
        .expect("activate task")
        .expect("active task");
    store
        .request_run_terminalization(&run.id, "interrupted", None)
        .await
        .expect("request terminalization");

    let provider = BlockingCancelProvider::new();
    let cancel_started = provider.cancel_started.clone();
    let release_cancel = provider.release_cancel.clone();
    let resumes = provider.resumes.clone();
    let service = RunService::with_provider(store.clone(), provider);
    let recovery = tokio::spawn(async move { service.recover_after_restart().await });
    tokio::time::timeout(Duration::from_secs(2), cancel_started.notified())
        .await
        .expect("cancellation did not start");

    assert!(
        store
            .run_recovery_lease(&run.id)
            .await
            .expect("read recovery lease")
            .is_none(),
        "terminalizing run must not enter recovery"
    );
    assert_eq!(resumes.load(Ordering::SeqCst), 0);

    release_cancel.notify_one();
    recovery
        .await
        .expect("join recovery")
        .expect("finish recovery");
    assert_eq!(store.run(&run.id).await.expect("run").status, "interrupted");
}

#[derive(Clone, Copy)]
struct RecoveringMockProvider;

#[async_trait]
impl Provider for RecoveringMockProvider {
    fn id(&self) -> &str {
        "mock"
    }

    async fn health(&self) -> ProviderHealth {
        MockProvider::new().health().await
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog> {
        MockProvider::new().catalog().await
    }

    async fn estimate(&self, req: ProviderRequest) -> ProviderResultValue<CostEstimate> {
        MockProvider::new().estimate(req).await
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        MockProvider::new().invoke(req).await
    }

    async fn resume(
        &self,
        _task: &DurableProviderTask,
        req: &ProviderRequest,
    ) -> ProviderResultValue<ProviderResume> {
        Ok(ProviderResume::Completed(self.invoke(req.clone()).await?))
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        MockProvider::new().cancel(handle).await
    }
}

#[derive(Clone)]
struct BlockingCancelProvider {
    cancel_started: Arc<Notify>,
    release_cancel: Arc<Notify>,
    resumes: Arc<AtomicUsize>,
}

impl BlockingCancelProvider {
    fn new() -> Self {
        Self {
            cancel_started: Arc::new(Notify::new()),
            release_cancel: Arc::new(Notify::new()),
            resumes: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait]
impl Provider for BlockingCancelProvider {
    fn id(&self) -> &str {
        "mock"
    }

    fn recovery_capabilities(&self, _provider_id: &str) -> ProviderRecoveryCapabilities {
        ProviderRecoveryCapabilities {
            resume: true,
            cancel: true,
        }
    }

    async fn health(&self) -> ProviderHealth {
        MockProvider::new().health().await
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog> {
        MockProvider::new().catalog().await
    }

    async fn estimate(&self, req: ProviderRequest) -> ProviderResultValue<CostEstimate> {
        MockProvider::new().estimate(req).await
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        MockProvider::new().invoke(req).await
    }

    async fn resume(
        &self,
        _task: &DurableProviderTask,
        _req: &ProviderRequest,
    ) -> ProviderResultValue<ProviderResume> {
        self.resumes.fetch_add(1, Ordering::SeqCst);
        Ok(ProviderResume::Pending {
            retry_after_ms: 1_000,
        })
    }

    async fn cancel(&self, _handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        self.cancel_started.notify_one();
        self.release_cancel.notified().await;
        Ok(())
    }
}

#[tokio::test]
async fn failed_paid_run_derives_retry_waiting_for_confirmation() {
    let (store, _dir) = open_self_heal_store().await;
    let (workspace_id, version_id) = self_heal_workspace_version(&store).await;
    let service = RunService::with_provider(store.clone(), CostedFailingProvider::paid());
    let mut events = service.events().subscribe();
    let pending = service
        .request_agent_run(AgentRunRequest {
            workspace_id: workspace_id.clone(),
            version_id,
            group_id: None,
            label: "Paid failure".to_owned(),
            provider: "mock".to_owned(),
            graph: self_heal_graph("writer-paid"),
        })
        .await
        .expect("prepare paid run");
    assert!(pending.estimate.amount > 0.0);

    service
        .start_confirmed_run(&pending.run.id)
        .await
        .expect("start confirmed parent");
    let retry = wait_for_retry(&store, &workspace_id).await;
    assert_eq!(retry.status, "waiting_confirmation");
    let ledger = store
        .cost_ledger_for_run(&retry.id)
        .await
        .expect("retry estimate ledger");
    assert!(
        ledger
            .iter()
            .any(|entry| entry.estimated && entry.amount > 0.0)
    );

    let saw_pending = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("retry event");
            if event.run_id == retry.id && event.ev == "run.retry_pending" {
                return true;
            }
        }
    })
    .await
    .expect("retry pending event timeout");
    assert!(saw_pending);
}

#[tokio::test]
async fn failed_recommended_sweep_run_derives_detached_retry() {
    let (store, _dir) = open_self_heal_store().await;
    let (workspace_id, version_id) = self_heal_workspace_version(&store).await;
    let service = RunService::with_provider(
        store.clone(),
        CostedFailingProvider::free_for("writer-recommended"),
    );
    let pending = service
        .request_sweep_plan(SweepPlan {
            workspace_id: workspace_id.clone(),
            version_id,
            label: "Failing sweep".to_owned(),
            provider: "mock".to_owned(),
            variants: vec![
                SweepVariant {
                    label: "baseline".to_owned(),
                    graph: self_heal_graph("writer-baseline"),
                },
                SweepVariant {
                    label: "recommended".to_owned(),
                    graph: self_heal_graph("writer-recommended"),
                },
            ],
        })
        .await
        .expect("prepare sweep");
    let parent_id = pending.runs[1].run.id.clone();
    let run_ids = pending
        .runs
        .iter()
        .map(|pending| pending.run.id.clone())
        .collect::<Vec<_>>();
    service
        .start_confirmed_sweep(&run_ids, &parent_id)
        .await
        .expect("start sweep");

    let retry = wait_for_retry(&store, &workspace_id).await;
    assert_eq!(retry.parent_run_id.as_deref(), Some(parent_id.as_str()));
    assert!(retry.group_id.is_none());
    assert_eq!(retry.trigger, "agent");
    wait_for_self_heal_status(&store, &retry.id, "failed").await;
}

#[tokio::test]
async fn retry_infrastructure_failure_emits_persisted_error_event() {
    let (store, _dir) = open_self_heal_store().await;
    let (workspace_id, version_id) = self_heal_workspace_version(&store).await;
    let service = RunService::new(store.clone());
    let mut events = service.events().subscribe();
    let run = store
        .create_run(NewRun {
            workspace_id: &workspace_id,
            version_id: &version_id,
            group_id: None,
            label: "Broken retry metadata",
            trigger: "agent",
            plan_json: None,
            estimate_json: Some("{invalid-json"),
            status: "failed",
        })
        .await
        .expect("failed run");

    service
        .continue_self_heal_from_failed(&run)
        .await
        .expect_err("invalid retry estimate must fail visibly");
    let event = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("event");
            if event.run_id == run.id && event.ev == "run.retry_failed" {
                return event;
            }
        }
    })
    .await
    .expect("retry_failed event timeout");
    assert!(
        event.data["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
    assert!(
        store
            .run_events(&run.id)
            .await
            .expect("persisted events")
            .iter()
            .any(|event| event.ev == "run.retry_failed")
    );
}

#[tokio::test]
async fn invalid_retry_environment_fails_before_creating_child_run() {
    if std::env::var_os(INVALID_RETRY_ENV_CHILD).is_none() {
        let mut command = std::process::Command::new(
            std::env::current_exe().expect("locate helixflow-run test executable"),
        );
        command
            .arg("--exact")
            .arg("self_heal_tests::invalid_retry_environment_fails_before_creating_child_run")
            .arg("--nocapture")
            .env(INVALID_RETRY_ENV_CHILD, "1");
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            command.env(
                "HELIXFLOW_RUN_MAX_RETRIES",
                std::ffi::OsString::from_vec(vec![0xff]),
            );
        }
        #[cfg(not(unix))]
        command.env("HELIXFLOW_RUN_MAX_RETRIES", "not-a-number");

        let output = command.output().expect("run isolated retry config test");
        assert!(
            output.status.success(),
            "isolated retry config test failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let (store, _dir) = open_self_heal_store().await;
    let (workspace_id, version_id) = self_heal_workspace_version(&store).await;
    let service = RunService::new(store.clone());
    let run = store
        .create_run(NewRun {
            workspace_id: &workspace_id,
            version_id: &version_id,
            group_id: None,
            label: "Invalid retry config",
            trigger: "agent",
            plan_json: None,
            estimate_json: Some(r#"{"amount":0.0,"currency":"USD","lines":[]}"#),
            status: "failed",
        })
        .await
        .expect("failed parent run");

    let error = service
        .continue_self_heal_from_failed(&run)
        .await
        .expect_err("invalid retry configuration must fail closed");
    assert!(error.to_string().contains("HELIXFLOW_RUN_MAX_RETRIES"));

    let latest = store
        .latest_workspace_run(&workspace_id)
        .await
        .expect("latest run")
        .expect("parent run remains latest");
    assert_eq!(
        latest.id, run.id,
        "invalid config must not create a child run"
    );
    assert!(
        store
            .run_events(&run.id)
            .await
            .expect("persisted retry events")
            .iter()
            .any(|event| event.ev == "run.retry_failed"),
        "invalid config must be visible as a persisted retry failure"
    );
}

#[derive(Clone)]
struct CostedFailingProvider {
    estimate_amount: f64,
    fail_node_id: Option<String>,
}

impl CostedFailingProvider {
    fn paid() -> Self {
        Self {
            estimate_amount: 1.0,
            fail_node_id: None,
        }
    }

    fn free_for(node_id: &str) -> Self {
        Self {
            estimate_amount: 0.0,
            fail_node_id: Some(node_id.to_owned()),
        }
    }
}

#[async_trait]
impl Provider for CostedFailingProvider {
    fn id(&self) -> &str {
        "mock"
    }

    async fn health(&self) -> ProviderHealth {
        ProviderHealth {
            ok: true,
            message: None,
        }
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog> {
        MockProvider::new().catalog().await
    }

    async fn estimate(&self, _req: ProviderRequest) -> ProviderResultValue<CostEstimate> {
        Ok(CostEstimate {
            amount: self.estimate_amount,
            currency: "USD".to_owned(),
            estimated: true,
            unknown: false,
        })
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        if self
            .fail_node_id
            .as_deref()
            .is_none_or(|node_id| node_id == req.node_id)
        {
            return Err(ProviderError::UnsupportedCapability(req.capability));
        }
        MockProvider::new().invoke(req).await
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        MockProvider::new().cancel(handle).await
    }
}

async fn open_self_heal_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let database_url = format!("sqlite://{}", dir.path().join("test.sqlite").display());
    let store = Store::open(&database_url).await.expect("store");
    (store, dir)
}

async fn self_heal_workspace_version(store: &Store) -> (String, String) {
    let workspace = store
        .create_workspace("Self heal")
        .await
        .expect("workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Graph",
            source: VersionSource::Manual,
            graph_path: "graph.json",
            graph_hash: "sha256:self-heal",
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("version");
    (workspace.id, version.id)
}

fn self_heal_graph(node_id: &str) -> WorkflowGraph {
    let input_id = format!("input-{node_id}");
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([
            (
                input_id.clone(),
                GraphNode {
                    node_type: "input.text".to_owned(),
                    title: "Input".to_owned(),
                    params: json!({ "text": "retry" }),
                    pos: [0.0, 0.0],
                    size: None,
                    semantics: None,
                },
            ),
            (
                node_id.to_owned(),
                GraphNode {
                    node_type: "llm.prompt_writer".to_owned(),
                    title: "Writer".to_owned(),
                    params: json!({ "style": "cinematic" }),
                    pos: [240.0, 0.0],
                    size: None,
                    semantics: None,
                },
            ),
        ]),
        edges: vec![GraphEdge {
            from: [input_id, "text".to_owned()],
            to: [node_id.to_owned(), "text".to_owned()],
            edge_type: "text".to_owned(),
        }],
        catalog_revision: None,
    }
}

async fn wait_for_retry(store: &Store, workspace_id: &str) -> RunRecord {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(run) = store
                .latest_workspace_run(workspace_id)
                .await
                .expect("latest run")
                && run.attempt > 0
            {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("retry run timeout")
}

async fn wait_for_self_heal_status(store: &Store, run_id: &str, expected: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if store.run(run_id).await.expect("run").status == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("run status timeout");
}
