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
        .create_workspace("Sweep workspace")
        .await
        .expect("create workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Executable graph",
            source: VersionSource::Manual,
            graph_path: "workspaces/ws_sweep/graphs/ver_sweep.json",
            graph_hash: "sha256:sweep",
            parent_id: None,
            semantics_json: None,
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
                    size: None,
                    semantics: None,
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
                    semantics: None,
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
                    size: None,
                    semantics: None,
                },
            ),
            (
                "save".to_owned(),
                GraphNode {
                    node_type: "output.save".to_owned(),
                    title: "Save".to_owned(),
                    params: json!({}),
                    pos: [720.0, 0.0],
                    size: None,
                    semantics: None,
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
        catalog_revision: None,
    }
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
}

#[async_trait]
impl Provider for BlockingProvider {
    fn id(&self) -> &str {
        "mock"
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
            self.blocked.notify_waiters();
            self.release.notified().await;
        }
        self.inner.invoke(req).await
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        self.release.notify_waiters();
        self.inner.cancel(handle).await
    }
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
impl Provider for CountingProvider {
    fn id(&self) -> &str {
        "mock"
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
        self.invokes.fetch_add(1, Ordering::SeqCst);
        self.inner.invoke(req).await
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        self.inner.cancel(handle).await
    }
}
