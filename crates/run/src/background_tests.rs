use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use helixflow_gateway::{
    CostEstimate, MockProvider, Provider, ProviderCatalog, ProviderHealth, ProviderRequest,
    ProviderResult, ProviderResultValue, ProviderTaskHandle,
};
use helixflow_graph::{GraphEdge, GraphNode, WorkflowGraph};
use helixflow_store::{NewVersion, Store, VersionSource};
use serde_json::json;
use tokio::sync::Notify;

use super::*;

async fn open_background_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("helixflow.sqlite");
    let database_url = format!("sqlite://{}", db_path.display());
    let store = Store::open(&database_url).await.expect("open store");
    (store, dir)
}

async fn background_workspace_version(store: &Store) -> (String, String) {
    let workspace = store
        .create_workspace("Background run workspace")
        .await
        .expect("create workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Executable graph",
            source: VersionSource::Manual,
            graph_path: "workspaces/ws_background/graphs/ver_run.json",
            graph_hash: "sha256:background",
            parent_id: None,
        })
        .await
        .expect("create version");
    (workspace.id, version.id)
}

fn background_graph() -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([
            (
                "text".to_owned(),
                GraphNode {
                    node_type: "input.text".to_owned(),
                    title: "Text".to_owned(),
                    params: json!({ "text": "launch teaser" }),
                    pos: [0.0, 0.0],
                    size: None,
                },
            ),
            (
                "writer".to_owned(),
                GraphNode {
                    node_type: "llm.prompt_writer".to_owned(),
                    title: "Prompt Writer".to_owned(),
                    params: json!({ "style": "cinematic" }),
                    pos: [240.0, 0.0],
                    size: None,
                },
            ),
            (
                "save".to_owned(),
                GraphNode {
                    node_type: "output.save".to_owned(),
                    title: "Save".to_owned(),
                    params: json!({}),
                    pos: [480.0, 0.0],
                    size: None,
                },
            ),
        ]),
        edges: vec![
            GraphEdge {
                from: ["text".to_owned(), "text".to_owned()],
                to: ["writer".to_owned(), "text".to_owned()],
                edge_type: "text".to_owned(),
            },
            GraphEdge {
                from: ["writer".to_owned(), "prompt".to_owned()],
                to: ["save".to_owned(), "artifact".to_owned()],
                edge_type: "artifact".to_owned(),
            },
        ],
    }
}

#[tokio::test]
async fn start_manual_run_returns_before_provider_finishes() {
    let (store, _dir) = open_background_store().await;
    let (workspace_id, version_id) = background_workspace_version(&store).await;
    let provider = BlockingProvider::default();
    let service = RunService::with_provider(store.clone(), provider.clone());

    let outcome = service
        .start_manual_run(ManualRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Background manual".to_owned(),
            provider: "mock".to_owned(),
            graph: background_graph(),
            force_rerun: false,
        })
        .await
        .expect("start manual run");

    assert_eq!(outcome.run.status, "queued");
    assert!(outcome.steps.iter().all(|step| step.state == "queued"));
    assert!(outcome.artifacts.is_empty());

    provider.wait_until_blocked().await;
    assert_eq!(
        store.run(&outcome.run.id).await.expect("run").status,
        "running"
    );
    service
        .interrupt_run(&outcome.run.id)
        .await
        .expect("interrupt manual run");
    provider.release();
    wait_for_status(&store, &outcome.run.id, "interrupted").await;
}

#[tokio::test]
async fn concurrent_confirms_claim_workspace_atomically() {
    let (store, _dir) = open_background_store().await;
    let (workspace_id, version_id) = background_workspace_version(&store).await;
    let provider = BlockingProvider::default();
    let service = RunService::with_provider(store.clone(), provider.clone());
    let pending_a = service
        .request_agent_run(AgentRunRequest {
            workspace_id: workspace_id.clone(),
            version_id: version_id.clone(),
            group_id: None,
            label: "Claim A".to_owned(),
            provider: "mock".to_owned(),
            graph: background_graph(),
        })
        .await
        .expect("request run a");
    let pending_b = service
        .request_agent_run(AgentRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Claim B".to_owned(),
            provider: "mock".to_owned(),
            graph: background_graph(),
        })
        .await
        .expect("request run b");

    let (result_a, result_b) = tokio::join!(
        service.start_confirmed_run(&pending_a.run.id),
        service.start_confirmed_run(&pending_b.run.id)
    );

    let winners = usize::from(result_a.is_ok()) + usize::from(result_b.is_ok());
    assert_eq!(
        winners, 1,
        "exactly one concurrent confirm may claim the workspace"
    );
    let (winner_id, loser) = if result_a.is_ok() {
        (pending_a.run.id.clone(), result_b)
    } else {
        (pending_b.run.id.clone(), result_a)
    };
    match loser.expect_err("loser must be rejected") {
        RunError::WorkspaceBusy { .. } | RunError::RunClaimContention(_) => {}
        other => panic!("unexpected loser error: {other}"),
    }

    provider.wait_until_blocked().await;
    service
        .interrupt_run(&winner_id)
        .await
        .expect("interrupt winner");
    provider.release();
    wait_for_status(&store, &winner_id, "interrupted").await;
}

#[tokio::test]
async fn prepare_manual_run_persists_force_rerun_and_estimate() {
    let (store, _dir) = open_background_store().await;
    let (workspace_id, version_id) = background_workspace_version(&store).await;
    let service = RunService::new(store.clone());

    let pending = service
        .prepare_manual_run(ManualRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Manual cost gate".to_owned(),
            provider: "mock".to_owned(),
            graph: background_graph(),
            force_rerun: true,
        })
        .await
        .expect("prepare manual run");

    assert_eq!(pending.run.status, "waiting_confirmation");
    assert!(pending.run.force_rerun);
    assert!(!pending.ledger.is_empty());
    assert!(pending.ledger.iter().all(|entry| entry.estimated));
}

#[tokio::test]
async fn start_confirmed_run_returns_before_provider_finishes() {
    let (store, _dir) = open_background_store().await;
    let (workspace_id, version_id) = background_workspace_version(&store).await;
    let provider = BlockingProvider::default();
    let service = RunService::with_provider(store.clone(), provider.clone());
    let pending = service
        .request_agent_run(AgentRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Background confirmed".to_owned(),
            provider: "mock".to_owned(),
            graph: background_graph(),
        })
        .await
        .expect("request run");

    let outcome = service
        .start_confirmed_run(&pending.run.id)
        .await
        .expect("start confirmed run");

    assert_eq!(outcome.run.status, "running");
    assert!(outcome.steps.iter().all(|step| step.state == "queued"));
    assert!(outcome.artifacts.is_empty());
    provider.wait_until_blocked().await;
    service
        .interrupt_run(&pending.run.id)
        .await
        .expect("interrupt confirmed run");
    provider.release();
    wait_for_status(&store, &pending.run.id, "interrupted").await;
}

#[tokio::test]
async fn background_success_event_is_emitted_after_actual_cost_ledger() {
    let (store, _dir) = open_background_store().await;
    let (workspace_id, version_id) = background_workspace_version(&store).await;
    let service = RunService::new(store.clone());
    let mut events = service.events().subscribe();
    let pending = service
        .request_agent_run(AgentRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Cost-before-success".to_owned(),
            provider: "mock".to_owned(),
            graph: background_graph(),
        })
        .await
        .expect("request run");

    service
        .start_confirmed_run(&pending.run.id)
        .await
        .expect("start confirmed run");
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
            .await
            .expect("terminal event timeout")
            .expect("run event");
        if event.run_id == pending.run.id && event.ev == "run.succeeded" {
            let ledger = store
                .cost_ledger_for_run(&pending.run.id)
                .await
                .expect("cost ledger at success event");
            assert!(ledger.iter().any(|entry| !entry.estimated));
            break;
        }
    }
}

#[tokio::test]
async fn start_confirmed_sweep_returns_queued_members_before_provider_finishes() {
    let (store, _dir) = open_background_store().await;
    let (workspace_id, version_id) = background_workspace_version(&store).await;
    let provider = BlockingProvider::default();
    let service = RunService::with_provider(store.clone(), provider.clone());
    let pending = service
        .request_sweep_plan(SweepPlan {
            workspace_id,
            version_id,
            label: "Background sweep".to_owned(),
            provider: "mock".to_owned(),
            variants: vec![
                SweepVariant {
                    label: "one".to_owned(),
                    graph: background_graph(),
                },
                SweepVariant {
                    label: "two".to_owned(),
                    graph: background_graph(),
                },
            ],
        })
        .await
        .expect("request sweep");
    let run_ids = pending
        .runs
        .iter()
        .map(|pending| pending.run.id.clone())
        .collect::<Vec<_>>();

    let outcome = service
        .start_confirmed_sweep(&run_ids, &run_ids[1])
        .await
        .expect("start sweep");

    assert!(outcome.runs.iter().all(|run| run.run.status == "queued"));
    assert!(outcome.artifacts.is_empty());
    provider.wait_until_blocked().await;
    service
        .interrupt_run(&run_ids[0])
        .await
        .expect("interrupt active sweep run");
    provider.release();
    wait_for_status(&store, &run_ids[0], "interrupted").await;
    wait_for_status(&store, &run_ids[1], "interrupted").await;
}

async fn wait_for_status(store: &Store, run_id: &str, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let run = store.run(run_id).await.expect("run");
        if run.status == expected {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "run status stayed {}",
            run.status
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[derive(Clone, Default)]
struct BlockingProvider {
    inner: MockProvider,
    blocked: Arc<Notify>,
    release: Arc<Notify>,
}

impl BlockingProvider {
    async fn wait_until_blocked(&self) {
        tokio::time::timeout(Duration::from_secs(2), self.blocked.notified())
            .await
            .expect("provider should block");
    }

    fn release(&self) {
        self.release.notify_waiters();
    }
}

#[async_trait]
impl Provider for BlockingProvider {
    fn id(&self) -> &str {
        self.inner.id()
    }

    async fn health(&self) -> ProviderHealth {
        self.inner.health().await
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog> {
        self.inner.catalog().await
    }

    async fn estimate(&self, req: ProviderRequest) -> ProviderResultValue<CostEstimate> {
        self.inner.estimate(req).await
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        if req.capability == "prompt_writer" {
            self.blocked.notify_waiters();
            self.release.notified().await;
        }
        self.inner.invoke(req).await
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        self.inner.cancel(handle).await
    }
}

/// Delegates to mock but reports an unknown estimate, like Atlas/FAL (HF-004).
#[derive(Clone, Default)]
struct UnknownCostProvider {
    inner: MockProvider,
    invoked: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait]
impl Provider for UnknownCostProvider {
    fn id(&self) -> &str {
        self.inner.id()
    }

    async fn health(&self) -> ProviderHealth {
        self.inner.health().await
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog> {
        self.inner.catalog().await
    }

    async fn estimate(&self, req: ProviderRequest) -> ProviderResultValue<CostEstimate> {
        let mut estimate = self.inner.estimate(req).await?;
        estimate.unknown = true;
        Ok(estimate)
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        self.invoked
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.inner.invoke(req).await
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        self.inner.cancel(handle).await
    }
}

#[tokio::test]
async fn manual_run_with_unknown_cost_waits_for_confirmation() {
    let (store, _dir) = open_background_store().await;
    let (workspace_id, version_id) = background_workspace_version(&store).await;
    let provider = UnknownCostProvider::default();
    let invoked = provider.invoked.clone();
    let service = RunService::with_provider(store.clone(), provider);

    let outcome = service
        .start_manual_run(ManualRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Unknown cost manual".to_owned(),
            provider: "mock".to_owned(),
            graph: background_graph(),
            force_rerun: false,
        })
        .await
        .expect("start manual run");

    assert_eq!(outcome.run.status, "waiting_confirmation");
    let estimate: crate::CostSummary =
        serde_json::from_str(outcome.run.estimate_json.as_deref().expect("estimate json"))
            .expect("estimate summary");
    assert!(estimate.unknown);
    assert!(
        !invoked.load(std::sync::atomic::Ordering::SeqCst),
        "provider must not be invoked before confirmation"
    );
    assert_eq!(
        store.run(&outcome.run.id).await.expect("run").status,
        "waiting_confirmation"
    );
}

#[tokio::test]
async fn manual_run_with_known_free_cost_still_starts_immediately() {
    let (store, _dir) = open_background_store().await;
    let (workspace_id, version_id) = background_workspace_version(&store).await;
    let service = RunService::with_provider(store.clone(), MockProvider::new());

    let outcome = service
        .start_manual_run(ManualRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Free manual".to_owned(),
            provider: "mock".to_owned(),
            graph: background_graph(),
            force_rerun: false,
        })
        .await
        .expect("start manual run");

    assert_eq!(outcome.run.status, "queued");
    wait_for_status(&store, &outcome.run.id, "succeeded").await;
}

#[test]
fn unknown_cost_always_requires_confirmation() {
    let summary = CostSummary {
        amount: 0.0,
        currency: "USD".to_owned(),
        estimated: true,
        unknown: true,
    };
    assert!(run_requires_confirmation(&summary).expect("confirmation decision"));
    let known_free = CostSummary {
        amount: 0.0,
        currency: "USD".to_owned(),
        estimated: true,
        unknown: false,
    };
    assert!(!run_requires_confirmation(&known_free).expect("confirmation decision"));
}

/// Blocks like BlockingProvider but exposes a remote handle and records
/// cancel calls (HF-011).
#[derive(Clone, Default)]
struct RemoteHandleProvider {
    inner: BlockingProvider,
    cancelled: Arc<std::sync::Mutex<Vec<ProviderTaskHandle>>>,
}

#[async_trait]
impl Provider for RemoteHandleProvider {
    fn id(&self) -> &str {
        self.inner.id()
    }

    async fn health(&self) -> ProviderHealth {
        self.inner.health().await
    }

    async fn active_handles(&self, run_id: &str) -> Vec<ProviderTaskHandle> {
        vec![ProviderTaskHandle {
            provider: "mock".to_owned(),
            provider_task_id: format!("remote-{run_id}"),
        }]
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog> {
        self.inner.catalog().await
    }

    async fn estimate(&self, req: ProviderRequest) -> ProviderResultValue<CostEstimate> {
        self.inner.estimate(req).await
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        self.inner.invoke(req).await
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        self.cancelled.lock().expect("cancel lock").push(handle);
        Ok(())
    }
}

#[tokio::test]
async fn interrupt_cancels_remote_provider_tasks() {
    let (store, _dir) = open_background_store().await;
    let (workspace_id, version_id) = background_workspace_version(&store).await;
    let provider = RemoteHandleProvider::default();
    let blocking = provider.inner.clone();
    let cancelled = provider.cancelled.clone();
    let service = RunService::with_provider(store.clone(), provider);
    let mut receiver = service.events().subscribe();

    let outcome = service
        .start_manual_run(ManualRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Remote cancel".to_owned(),
            provider: "mock".to_owned(),
            graph: background_graph(),
            force_rerun: false,
        })
        .await
        .expect("start manual run");
    blocking.wait_until_blocked().await;

    service
        .interrupt_run(&outcome.run.id)
        .await
        .expect("interrupt run");

    let cancelled = cancelled.lock().expect("cancel lock").clone();
    assert_eq!(cancelled.len(), 1);
    assert_eq!(
        cancelled[0].provider_task_id,
        format!("remote-{}", outcome.run.id)
    );
    let mut saw_remote_cancel_event = false;
    while let Ok(event) = receiver.try_recv() {
        if event.ev == "run.remote_cancelled" {
            saw_remote_cancel_event = true;
        }
    }
    assert!(
        saw_remote_cancel_event,
        "remote cancel event must be emitted"
    );

    blocking.release();
    wait_for_status(&store, &outcome.run.id, "interrupted").await;
}

/// Like RemoteHandleProvider, but the remote API has no cancel endpoint
/// (the real Atlas provider) — cancel always fails with CancelUnsupported.
#[derive(Clone, Default)]
struct UncancellableRemoteProvider {
    inner: BlockingProvider,
}

#[async_trait]
impl Provider for UncancellableRemoteProvider {
    fn id(&self) -> &str {
        self.inner.id()
    }

    async fn health(&self) -> ProviderHealth {
        self.inner.health().await
    }

    async fn active_handles(&self, run_id: &str) -> Vec<ProviderTaskHandle> {
        vec![ProviderTaskHandle {
            provider: "mock".to_owned(),
            provider_task_id: format!("remote-{run_id}"),
        }]
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog> {
        self.inner.catalog().await
    }

    async fn estimate(&self, req: ProviderRequest) -> ProviderResultValue<CostEstimate> {
        self.inner.estimate(req).await
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        self.inner.invoke(req).await
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        Err(helixflow_gateway::ProviderError::CancelUnsupported(
            handle.provider_task_id,
        ))
    }
}

#[tokio::test]
async fn interrupt_surfaces_unsupported_remote_cancel() {
    let (store, _dir) = open_background_store().await;
    let (workspace_id, version_id) = background_workspace_version(&store).await;
    let provider = UncancellableRemoteProvider::default();
    let blocking = provider.inner.clone();
    let service = RunService::with_provider(store.clone(), provider);
    let mut receiver = service.events().subscribe();

    let outcome = service
        .start_manual_run(ManualRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Remote cancel unsupported".to_owned(),
            provider: "mock".to_owned(),
            graph: background_graph(),
            force_rerun: false,
        })
        .await
        .expect("start manual run");
    blocking.wait_until_blocked().await;

    service
        .interrupt_run(&outcome.run.id)
        .await
        .expect("interrupt run");

    let mut unsupported_event = None;
    let mut saw_cancelled_event = false;
    while let Ok(event) = receiver.try_recv() {
        match event.ev.as_str() {
            "run.remote_cancel_unsupported" => unsupported_event = Some(event),
            "run.remote_cancelled" => saw_cancelled_event = true,
            _ => {}
        }
    }
    let unsupported_event =
        unsupported_event.expect("run.remote_cancel_unsupported event must be emitted");
    assert!(
        !saw_cancelled_event,
        "must not claim the remote task was cancelled"
    );
    let message = unsupported_event
        .data
        .get("message")
        .and_then(serde_json::Value::as_str)
        .expect("unsupported event carries a user-facing message");
    assert!(message.contains("incur charges"));
    assert_eq!(
        unsupported_event
            .data
            .get("provider_task_id")
            .and_then(serde_json::Value::as_str),
        Some(format!("remote-{}", outcome.run.id).as_str())
    );

    blocking.release();
    wait_for_status(&store, &outcome.run.id, "interrupted").await;
}
