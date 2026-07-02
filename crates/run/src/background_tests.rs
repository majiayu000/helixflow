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
                "save".to_owned(),
                GraphNode {
                    node_type: "output.save".to_owned(),
                    title: "Save".to_owned(),
                    params: json!({}),
                    pos: [480.0, 0.0],
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
