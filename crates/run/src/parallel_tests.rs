use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use helixflow_gateway::{
    CostEstimate, MockProvider, Provider, ProviderCatalog, ProviderRequest, ProviderResult,
    ProviderResultValue, ProviderTaskHandle,
};
use helixflow_graph::{GraphEdge, GraphNode, WorkflowGraph};
use helixflow_store::{NewVersion, Store, VersionSource};
use serde_json::json;
use tokio::sync::{Mutex, Notify, Semaphore};

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
        .create_workspace("Parallel workspace")
        .await
        .expect("create workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Parallel graph",
            source: VersionSource::Manual,
            graph_path: "workspaces/ws_parallel/graphs/ver_parallel.json",
            graph_hash: "sha256:parallel",
            parent_id: None,
        })
        .await
        .expect("create version");
    (workspace.id, version.id)
}

fn parallel_branches_graph() -> WorkflowGraph {
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
                "writer_a".to_owned(),
                GraphNode {
                    node_type: "llm.prompt_writer".to_owned(),
                    title: "Prompt A".to_owned(),
                    params: json!({ "style": "cinematic" }),
                    pos: [240.0, -80.0],
                    size: None,
                },
            ),
            (
                "writer_b".to_owned(),
                GraphNode {
                    node_type: "llm.prompt_writer".to_owned(),
                    title: "Prompt B".to_owned(),
                    params: json!({ "style": "product" }),
                    pos: [240.0, 80.0],
                    size: None,
                },
            ),
        ]),
        edges: vec![
            GraphEdge {
                from: ["text".to_owned(), "text".to_owned()],
                to: ["writer_a".to_owned(), "text".to_owned()],
                edge_type: "text".to_owned(),
            },
            GraphEdge {
                from: ["text".to_owned(), "text".to_owned()],
                to: ["writer_b".to_owned(), "text".to_owned()],
                edge_type: "text".to_owned(),
            },
        ],
    }
}

fn manual_request(workspace_id: &str, version_id: &str) -> ManualRunRequest {
    ManualRunRequest {
        workspace_id: workspace_id.to_owned(),
        version_id: version_id.to_owned(),
        group_id: None,
        label: "Parallel run".to_owned(),
        provider: "mock".to_owned(),
        graph: parallel_branches_graph(),
        force_rerun: false,
    }
}

#[tokio::test]
async fn ready_independent_steps_run_concurrently_when_limit_allows() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let provider = BlockingParallelProvider::default();
    let service =
        RunService::with_provider(store.clone(), provider.clone()).with_max_parallel_steps(2);
    let runner = service.clone();
    let request = manual_request(&workspace_id, &version_id);

    let handle = tokio::spawn(async move { runner.execute_manual_run(request).await });
    tokio::time::timeout(Duration::from_secs(2), provider.wait_until_started(2))
        .await
        .expect("both provider branches should start before release");
    assert_eq!(provider.started_count(), 2);

    provider.release(2);
    let outcome = tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("parallel run should finish")
        .expect("join run")
        .expect("run outcome");
    let events = store.run_events(&outcome.run.id).await.expect("events");

    assert_eq!(outcome.run.status, "succeeded");
    assert!(outcome.steps.iter().all(|step| step.state == "succeeded"));
    assert_eq!(provider.started_count(), 2);
    assert_eq!(
        events.iter().map(|event| event.seq).collect::<Vec<_>>(),
        (1..=events.len() as i64).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn max_parallel_steps_one_runs_ready_steps_serially() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let provider = BlockingParallelProvider::default();
    let service = RunService::with_provider(store, provider.clone()).with_max_parallel_steps(1);
    let runner = service.clone();
    let request = manual_request(&workspace_id, &version_id);

    let handle = tokio::spawn(async move { runner.execute_manual_run(request).await });
    tokio::time::timeout(Duration::from_secs(2), provider.wait_until_started(1))
        .await
        .expect("first provider branch should start");
    assert_eq!(provider.started_count(), 1);

    provider.release(1);
    tokio::time::timeout(Duration::from_secs(2), provider.wait_until_started(2))
        .await
        .expect("second provider branch should start after first release");
    provider.release(1);

    let outcome = tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("serial run should finish")
        .expect("join run")
        .expect("run outcome");
    assert_eq!(outcome.run.status, "succeeded");
    assert_eq!(provider.started_count(), 2);
}

#[tokio::test]
async fn interrupt_cancels_all_in_flight_parallel_steps() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let provider = BlockingParallelProvider::default();
    let service = RunService::with_provider(store, provider.clone()).with_max_parallel_steps(2);
    let runner = service.clone();
    let request = manual_request(&workspace_id, &version_id);

    let handle = tokio::spawn(async move { runner.execute_manual_run(request).await });
    tokio::time::timeout(Duration::from_secs(2), provider.wait_until_started(2))
        .await
        .expect("both branches should be in flight");
    let run_id = provider.run_id().await.expect("run id");
    service.interrupt_run(&run_id).await.expect("interrupt run");

    let outcome = tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("interrupted run should finish")
        .expect("join run")
        .expect("run outcome");

    assert_eq!(outcome.run.status, "interrupted");
    assert!(
        outcome
            .steps
            .iter()
            .filter(|step| step.node_id.starts_with("writer_"))
            .all(|step| step.state == "skipped")
    );
    assert!(outcome.steps.iter().all(|step| step.state != "running"));
}

#[derive(Clone)]
struct BlockingParallelProvider {
    inner: MockProvider,
    started: Arc<AtomicUsize>,
    started_notify: Arc<Notify>,
    release: Arc<Semaphore>,
    run_id: Arc<Mutex<Option<String>>>,
}

impl Default for BlockingParallelProvider {
    fn default() -> Self {
        Self {
            inner: MockProvider::new(),
            started: Arc::new(AtomicUsize::new(0)),
            started_notify: Arc::new(Notify::new()),
            release: Arc::new(Semaphore::new(0)),
            run_id: Arc::new(Mutex::new(None)),
        }
    }
}

impl BlockingParallelProvider {
    fn started_count(&self) -> usize {
        self.started.load(Ordering::SeqCst)
    }

    async fn wait_until_started(&self, expected: usize) {
        loop {
            let notified = self.started_notify.notified();
            if self.started_count() >= expected {
                return;
            }
            notified.await;
        }
    }

    fn release(&self, permits: usize) {
        self.release.add_permits(permits);
    }

    async fn run_id(&self) -> Option<String> {
        self.run_id.lock().await.clone()
    }
}

#[async_trait]
impl Provider for BlockingParallelProvider {
    fn id(&self) -> &str {
        self.inner.id()
    }

    async fn health(&self) -> helixflow_gateway::ProviderHealth {
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
            *self.run_id.lock().await = Some(req.run_id.clone());
            self.started.fetch_add(1, Ordering::SeqCst);
            self.started_notify.notify_waiters();
            let permit = self.release.acquire().await.expect("release permit");
            drop(permit);
        }
        self.inner.invoke(req).await
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        self.inner.cancel(handle).await
    }
}
