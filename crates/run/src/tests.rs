use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use helixflow_gateway::{
    ArtifactContent, ArtifactKind, ArtifactPayload, CostEstimate, MockProvider, Provider,
    ProviderCatalog, ProviderError, ProviderHealth, ProviderRequest, ProviderResult,
    ProviderResultValue, ProviderTaskHandle,
};
use helixflow_graph::{GraphEdge, GraphNode, WorkflowGraph};
use helixflow_store::{NewVersion, Store, VersionSource};
use serde_json::json;
use tokio::sync::Notify;

use super::*;

async fn open_temp_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("helixflow.sqlite");
    let database_url = format!("sqlite://{}", db_path.display());
    let store = Store::open(&database_url).await.expect("open store");
    (store, dir)
}

async fn workspace_version(store: &Store) -> (String, String) {
    let workspace = store
        .create_workspace("Run workspace")
        .await
        .expect("create workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Executable graph",
            source: VersionSource::Manual,
            graph_path: "workspaces/ws_run/graphs/ver_run.json",
            graph_hash: "sha256:run",
            parent_id: None,
        })
        .await
        .expect("create version");
    (workspace.id, version.id)
}

fn executable_graph() -> WorkflowGraph {
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
                },
            ),
            (
                "writer".to_owned(),
                GraphNode {
                    node_type: "llm.prompt_writer".to_owned(),
                    title: "Prompt Writer".to_owned(),
                    params: json!({ "style": "cinematic" }),
                    pos: [240.0, 0.0],
                },
            ),
            (
                "video".to_owned(),
                GraphNode {
                    node_type: "video.text_to_video".to_owned(),
                    title: "Video".to_owned(),
                    params: json!({
                        "prompt": "launch teaser",
                        "duration_sec": 4,
                        "aspect_ratio": "9:16"
                    }),
                    pos: [480.0, 0.0],
                },
            ),
            (
                "save".to_owned(),
                GraphNode {
                    node_type: "output.save".to_owned(),
                    title: "Save".to_owned(),
                    params: json!({}),
                    pos: [720.0, 0.0],
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
                to: ["video".to_owned(), "prompt".to_owned()],
                edge_type: "text".to_owned(),
            },
            GraphEdge {
                from: ["video".to_owned(), "video".to_owned()],
                to: ["save".to_owned(), "artifact".to_owned()],
                edge_type: "artifact".to_owned(),
            },
        ],
    }
}

#[test]
fn reports_module_name() {
    assert_eq!(module_name(), "run");
}

#[test]
fn serializes_run_status_boundary() {
    let encoded = serde_json::to_value(RunStatus::Interrupted).expect("serialize run status");

    assert_eq!(encoded, "interrupted");
}

#[tokio::test]
async fn run_service_can_share_an_injected_event_bus() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let events = EventBus::new(16);
    let service = RunService::with_provider_and_events(store, MockProvider::new(), events.clone());
    let mut receiver = events.subscribe();

    let outcome = service
        .execute_manual_run(ManualRunRequest {
            workspace_id: workspace_id.clone(),
            version_id,
            group_id: None,
            label: "Shared bus test".to_owned(),
            provider: "mock".to_owned(),
            graph: executable_graph(),
        })
        .await
        .expect("execute run");

    let mut streamed = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        streamed.push(event);
    }

    assert_eq!(outcome.run.status, "succeeded");
    assert!(
        streamed
            .iter()
            .any(|event| event.workspace_id == workspace_id && event.ev == "node.state")
    );
}

#[tokio::test]
async fn manual_run_persists_steps_events_and_artifacts() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let service = RunService::new(store.clone());
    let mut receiver = service.events().subscribe();

    let outcome = service
        .execute_manual_run(ManualRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Manual test".to_owned(),
            provider: "mock".to_owned(),
            graph: executable_graph(),
        })
        .await
        .expect("execute run");

    assert_eq!(outcome.run.status, "succeeded");
    assert_eq!(outcome.steps.len(), 4);
    assert!(outcome.steps.iter().all(|step| step.state == "succeeded"));
    assert!(
        outcome
            .artifacts
            .iter()
            .any(|artifact| artifact.kind == "video")
    );
    assert!(outcome.artifacts.iter().any(|artifact| artifact.selected));

    let events = store
        .run_events(&outcome.run.id)
        .await
        .expect("stored events");
    assert!(events.iter().any(|event| event.ev == "node.state"));
    assert!(events.iter().any(|event| event.ev == "run.succeeded"));

    let mut streamed = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        streamed.push(event);
    }
    assert!(streamed.iter().any(|event| event.ev == "node.state"));
}

#[tokio::test]
async fn provider_artifact_content_persists_relative_file_path() {
    let dir = tempfile::tempdir().expect("temp dir");
    let payload = ArtifactPayload {
        kind: ArtifactKind::Image,
        mime: "image/png".to_owned(),
        storage_uri: String::new(),
        content: ArtifactContent::InlineBytes {
            bytes: vec![137, 80, 78, 71],
            ext_hint: Some("png".to_owned()),
        },
        width: Some(1),
        height: Some(1),
        duration_ms: None,
        meta: json!({}),
    };

    let relative = persist_provider_artifact(dir.path(), "run_1", "step_1", "image", &payload)
        .await
        .expect("persist artifact");

    assert!(relative.starts_with("artifacts/run_1/"));
    assert!(!relative.starts_with("http"));
    assert_eq!(
        tokio::fs::read(dir.path().join(&relative))
            .await
            .expect("read artifact"),
        vec![137, 80, 78, 71],
    );
}

#[tokio::test]
async fn provider_artifact_remote_url_requires_https() {
    let dir = tempfile::tempdir().expect("temp dir");
    let payload = ArtifactPayload {
        kind: ArtifactKind::Image,
        mime: "image/png".to_owned(),
        storage_uri: String::new(),
        content: ArtifactContent::RemoteUrl {
            url: "http://127.0.0.1/image.png".to_owned(),
        },
        width: Some(1),
        height: Some(1),
        duration_ms: None,
        meta: json!({}),
    };

    let err = persist_provider_artifact(dir.path(), "run_1", "step_1", "image", &payload)
        .await
        .expect_err("reject non-https remote artifact");

    assert!(
        err.to_string()
            .contains("remote artifact URL must use https")
    );
}

#[tokio::test]
async fn interrupt_skips_remaining_steps() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let provider = BlockingProvider::default();
    let service = RunService::with_provider(store, provider.clone());
    let runner = service.clone();
    let mut receiver = service.events().subscribe();

    let handle = tokio::spawn(async move {
        runner
            .execute_manual_run(ManualRunRequest {
                workspace_id,
                version_id,
                group_id: None,
                label: "Interrupt test".to_owned(),
                provider: "mock".to_owned(),
                graph: executable_graph(),
            })
            .await
    });

    provider.wait_until_blocked().await;
    let run_id = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = receiver.recv().await.expect("receive event");
            if event.ev == "node.state"
                && event.data["node_id"] == "writer"
                && event.data["state"] == "running"
            {
                break event.run_id;
            }
        }
    })
    .await
    .expect("writer running event");

    service.interrupt_run(&run_id).await.expect("interrupt run");
    let outcome = tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("run should stop after interrupt")
        .expect("join run")
        .expect("run outcome");

    assert_eq!(outcome.run.status, "interrupted");
    assert_eq!(
        outcome
            .steps
            .iter()
            .find(|step| step.node_id == "writer")
            .expect("writer step")
            .state,
        "skipped"
    );
    assert_eq!(
        outcome
            .steps
            .iter()
            .find(|step| step.node_id == "video")
            .expect("video step")
            .state,
        "skipped"
    );
    assert!(
        outcome
            .steps
            .iter()
            .filter(|step| step.state == "skipped")
            .count()
            >= 1
    );
}

#[tokio::test]
async fn failed_step_skips_downstream_steps() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let service = RunService::with_provider(store.clone(), FailingProvider);
    let mut receiver = service.events().subscribe();

    let err = service
        .execute_manual_run(ManualRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Failure test".to_owned(),
            provider: "mock".to_owned(),
            graph: executable_graph(),
        })
        .await
        .expect_err("provider failure should fail run");

    assert!(err.to_string().contains("unsupported provider capability"));
    let mut run_id = None;
    while let Ok(event) = receiver.try_recv() {
        if event.ev == "run.failed" {
            run_id = Some(event.run_id);
        }
    }
    let run_id = run_id.expect("run.failed event");
    let steps = store.run_steps(&run_id).await.expect("run steps");

    assert_eq!(
        steps
            .iter()
            .find(|step| step.node_id == "writer")
            .expect("writer step")
            .state,
        "failed"
    );
    assert_eq!(
        steps
            .iter()
            .find(|step| step.node_id == "video")
            .expect("video step")
            .state,
        "skipped"
    );
}

#[tokio::test]
async fn agent_requested_run_waits_for_confirmation_and_records_costs() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let provider = CountingProvider::default();
    let service = RunService::with_provider(store.clone(), provider.clone());
    let mut receiver = service.events().subscribe();

    let pending = service
        .request_agent_run(AgentRunRequest {
            workspace_id: workspace_id.clone(),
            version_id,
            group_id: None,
            label: "Agent requested run".to_owned(),
            provider: "mock".to_owned(),
            graph: executable_graph(),
        })
        .await
        .expect("request agent run");

    assert_eq!(pending.run.status, "waiting_confirmation");
    assert_eq!(provider.invoke_count(), 0);
    assert!(pending.estimate.estimated);
    assert!(pending.ledger.iter().all(|entry| entry.estimated));
    assert!(
        store
            .cost_ledger_for_run(&pending.run.id)
            .await
            .expect("estimate ledger")
            .iter()
            .all(|entry| entry.estimated)
    );

    let mut events = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        events.push(event);
    }
    assert!(events.iter().any(|event| event.ev == "run.requested"));
    assert!(!events.iter().any(|event| event.ev == "run.started"));

    let outcome = service
        .confirm_run(&pending.run.id)
        .await
        .expect("confirm run");

    assert_eq!(outcome.run.status, "succeeded");
    assert!(provider.invoke_count() > 0);
    let ledger = store
        .cost_ledger_for_run(&pending.run.id)
        .await
        .expect("cost ledger");
    assert!(ledger.iter().any(|entry| entry.estimated));
    assert!(ledger.iter().any(|entry| !entry.estimated));
}

#[tokio::test]
async fn holding_pending_run_marks_interrupted_without_invoking_provider() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let provider = CountingProvider::default();
    let service = RunService::with_provider(store.clone(), provider.clone());
    let mut receiver = service.events().subscribe();
    let pending = service
        .request_agent_run(AgentRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Hold requested run".to_owned(),
            provider: "mock".to_owned(),
            graph: executable_graph(),
        })
        .await
        .expect("request agent run");

    let outcome = service.hold_run(&pending.run.id).await.expect("hold run");

    assert_eq!(outcome.run.status, "interrupted");
    assert_eq!(provider.invoke_count(), 0);
    assert!(
        service
            .confirm_run(&pending.run.id)
            .await
            .expect_err("held run cannot be confirmed")
            .to_string()
            .contains("expected status `waiting_confirmation`")
    );
    let mut events = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        events.push(event);
    }
    assert!(events.iter().any(|event| event.ev == "run.requested"));
    assert!(events.iter().any(|event| event.ev == "run.interrupted"));
    assert!(!events.iter().any(|event| event.ev == "run.started"));
}

#[tokio::test]
async fn concurrent_confirm_does_not_double_invoke_provider() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let provider = BlockingProvider::default();
    let service = RunService::with_provider(store, provider.clone());
    let pending = service
        .request_agent_run(AgentRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Concurrent confirmation".to_owned(),
            provider: "mock".to_owned(),
            graph: executable_graph(),
        })
        .await
        .expect("request agent run");
    let run_id = pending.run.id.clone();
    let runner = service.clone();
    let first = tokio::spawn(async move { runner.confirm_run(&run_id).await });

    provider.wait_until_blocked().await;
    let err = service
        .confirm_run(&pending.run.id)
        .await
        .expect_err("second confirmation should fail while first is running");

    assert!(
        err.to_string()
            .contains("expected status `waiting_confirmation`")
    );
    provider.release();
    let outcome = tokio::time::timeout(Duration::from_secs(2), first)
        .await
        .expect("first confirmation finishes")
        .expect("join first confirmation")
        .expect("first confirmation outcome");
    assert_eq!(outcome.run.status, "succeeded");
}

#[tokio::test]
async fn failed_partial_run_still_records_actual_costs() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let service = RunService::with_provider(store.clone(), FailsOnVideoProvider::default());
    let pending = service
        .request_agent_run(AgentRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Partial failure".to_owned(),
            provider: "mock".to_owned(),
            graph: executable_graph(),
        })
        .await
        .expect("request agent run");

    let err = service
        .confirm_run(&pending.run.id)
        .await
        .expect_err("video provider failure should fail run");

    assert!(err.to_string().contains("unsupported provider capability"));
    let ledger = store
        .cost_ledger_for_run(&pending.run.id)
        .await
        .expect("cost ledger");
    assert!(ledger.iter().any(|entry| entry.estimated));
    assert!(ledger.iter().any(|entry| !entry.estimated));
}

#[tokio::test]
async fn sweep_plan_produces_multiple_outputs_and_selected_recommendation() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let service = RunService::new(store);

    let pending = service
        .request_sweep_plan(SweepPlan {
            workspace_id,
            version_id,
            label: "Prompt sweep".to_owned(),
            provider: "mock".to_owned(),
            variants: vec![
                SweepVariant {
                    label: "cinematic".to_owned(),
                    graph: executable_graph(),
                },
                SweepVariant {
                    label: "direct".to_owned(),
                    graph: executable_graph(),
                },
            ],
        })
        .await
        .expect("request sweep");
    let run_ids: Vec<String> = pending.runs.iter().map(|run| run.run.id.clone()).collect();

    assert_eq!(pending.runs.len(), 2);
    assert_eq!(
        pending.group_id,
        pending.runs[0].run.group_id.clone().expect("group")
    );
    assert!(pending.estimate.estimated);

    let outcome = service
        .confirm_sweep_runs(&run_ids, &run_ids[1])
        .await
        .expect("confirm sweep");

    assert_eq!(outcome.runs.len(), 2);
    assert_eq!(outcome.group_id, pending.group_id);
    assert!(outcome.artifacts.len() >= 2);
    let recommendation = outcome.recommendation.as_ref().expect("recommendation");
    assert!(recommendation.selected);
    assert_eq!(recommendation.run_id.as_deref(), Some(run_ids[1].as_str()));
    assert_eq!(
        outcome
            .artifacts
            .iter()
            .filter(|artifact| artifact.selected)
            .count(),
        1
    );
}

#[tokio::test]
async fn interrupted_sweep_marks_remaining_runs_interrupted_without_recommendation() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let provider = BlockingProvider::default();
    let service = RunService::with_provider(store.clone(), provider.clone());
    let pending = service
        .request_sweep_plan(SweepPlan {
            workspace_id,
            version_id,
            label: "Prompt sweep".to_owned(),
            provider: "mock".to_owned(),
            variants: vec![
                SweepVariant {
                    label: "one".to_owned(),
                    graph: executable_graph(),
                },
                SweepVariant {
                    label: "two".to_owned(),
                    graph: executable_graph(),
                },
            ],
        })
        .await
        .expect("request sweep");
    let run_ids: Vec<String> = pending.runs.iter().map(|run| run.run.id.clone()).collect();
    let runner = service.clone();
    let confirm_ids = run_ids.clone();
    let recommended_run_id = run_ids[1].clone();
    let handle = tokio::spawn(async move {
        runner
            .confirm_sweep_runs(&confirm_ids, &recommended_run_id)
            .await
    });

    provider.wait_until_blocked().await;
    service
        .interrupt_run(&run_ids[0])
        .await
        .expect("interrupt active sweep run");
    let outcome = tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("sweep confirmation should stop after interrupt")
        .expect("join sweep")
        .expect("sweep outcome");

    assert_eq!(outcome.recommendation, None);
    assert!(outcome.artifacts.iter().all(|artifact| !artifact.selected));
    for run_id in run_ids {
        let run = store.run(&run_id).await.expect("sweep run");
        assert_eq!(run.status, "interrupted");
    }
}

#[tokio::test]
async fn invalid_sweep_confirmation_does_not_invoke_provider() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let provider = CountingProvider::default();
    let service = RunService::with_provider(store, provider.clone());
    let pending = service
        .request_sweep_plan(SweepPlan {
            workspace_id,
            version_id,
            label: "Prompt sweep".to_owned(),
            provider: "mock".to_owned(),
            variants: vec![
                SweepVariant {
                    label: "one".to_owned(),
                    graph: executable_graph(),
                },
                SweepVariant {
                    label: "two".to_owned(),
                    graph: executable_graph(),
                },
            ],
        })
        .await
        .expect("request sweep");
    let duplicate = pending.runs[0].run.id.clone();
    let err = service
        .confirm_sweep_runs(&[duplicate.clone(), duplicate.clone()], &duplicate)
        .await
        .expect_err("duplicate sweep ids should fail before execution");

    assert!(err.to_string().contains("unique"));
    assert_eq!(provider.invoke_count(), 0);
}

#[derive(Clone, Default)]
struct BlockingProvider {
    inner: MockProvider,
    blocked: Arc<Notify>,
    release: Arc<Notify>,
}

impl BlockingProvider {
    async fn wait_until_blocked(&self) {
        self.blocked.notified().await;
    }

    fn release(&self) {
        self.release.notify_waiters();
    }
}

#[derive(Clone)]
struct FailingProvider;

#[derive(Clone, Default)]
struct FailsOnVideoProvider {
    inner: MockProvider,
}

#[derive(Clone, Default)]
struct CountingProvider {
    inner: MockProvider,
    invokes: Arc<AtomicUsize>,
}

impl CountingProvider {
    fn invoke_count(&self) -> usize {
        self.invokes.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Provider for FailingProvider {
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
            amount: 0.0,
            currency: "USD".to_owned(),
            estimated: true,
        })
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        Err(ProviderError::UnsupportedCapability(req.capability))
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        MockProvider::new().cancel(handle).await
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

#[async_trait]
impl Provider for FailsOnVideoProvider {
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
        if req.capability == "text_to_video" {
            return Err(ProviderError::UnsupportedCapability(req.capability));
        }
        self.inner.invoke(req).await
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        self.inner.cancel(handle).await
    }
}

#[async_trait]
impl Provider for CountingProvider {
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
        self.invokes.fetch_add(1, Ordering::SeqCst);
        self.inner.invoke(req).await
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        self.inner.cancel(handle).await
    }
}
