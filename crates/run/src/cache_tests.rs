use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use helixflow_gateway::{
    CostEstimate, MockProvider, Provider, ProviderCatalog, ProviderRequest, ProviderResult,
    ProviderResultValue, ProviderTaskHandle,
};
use helixflow_graph::{GraphEdge, GraphNode, WorkflowGraph};
use helixflow_store::{NewVersion, RunStepRecord, Store, VersionSource};
use serde_json::json;

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
        .create_workspace("Run cache workspace")
        .await
        .expect("create workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Executable graph",
            source: VersionSource::Manual,
            graph_path: "workspaces/ws_cache/graphs/ver_cache.json",
            graph_hash: "sha256:cache",
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

fn manual_request(
    workspace_id: &str,
    version_id: &str,
    provider: &str,
    graph: WorkflowGraph,
    force_rerun: bool,
) -> ManualRunRequest {
    ManualRunRequest {
        workspace_id: workspace_id.to_owned(),
        version_id: version_id.to_owned(),
        group_id: None,
        label: "Manual cache test".to_owned(),
        provider: provider.to_owned(),
        graph,
        force_rerun,
    }
}

fn node_step<'a>(outcome: &'a RunOutcome, node_id: &str) -> &'a RunStepRecord {
    outcome
        .steps
        .iter()
        .find(|step| step.node_id == node_id)
        .expect("node step")
}

fn step_cached(step: &RunStepRecord) -> bool {
    step.metadata_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|value| value.get("cached").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

#[tokio::test]
async fn node_cache_reuses_clean_graph_without_provider_invokes() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let provider = CountingProvider::default();
    let service = RunService::with_provider(store.clone(), provider.clone());

    let first = service
        .execute_manual_run(manual_request(
            &workspace_id,
            &version_id,
            "mock",
            executable_graph(),
            false,
        ))
        .await
        .expect("first run");
    let invokes_after_first = provider.invoke_count();
    let second = service
        .execute_manual_run(manual_request(
            &workspace_id,
            &version_id,
            "mock",
            executable_graph(),
            false,
        ))
        .await
        .expect("second run");

    assert_eq!(invokes_after_first, 2);
    assert_eq!(provider.invoke_count(), invokes_after_first);
    assert!(first.steps.iter().all(|step| !step_cached(step)));
    assert!(second.steps.iter().all(step_cached));
    assert!(second.artifacts.iter().any(|artifact| artifact.selected));
}

#[tokio::test]
async fn rejected_cached_artifact_forces_provider_execution() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let provider = CountingProvider::default();
    let service = RunService::with_provider(store.clone(), provider.clone());
    let first = service
        .execute_manual_run(manual_request(
            &workspace_id,
            &version_id,
            "mock",
            executable_graph(),
            false,
        ))
        .await
        .expect("first run");
    let first_count = provider.invoke_count();
    for artifact in first.artifacts {
        store
            .set_artifact_review_state(&artifact.id, &["pending"], "rejected")
            .await
            .expect("reject cached artifact");
    }

    service
        .execute_manual_run(manual_request(
            &workspace_id,
            &version_id,
            "mock",
            executable_graph(),
            false,
        ))
        .await
        .expect("rerun after rejection");

    assert!(provider.invoke_count() > first_count);
}

#[tokio::test]
async fn node_cache_reruns_terminal_dirty_subgraph_only() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let provider = CountingProvider::default();
    let service = RunService::with_provider(store, provider.clone());

    service
        .execute_manual_run(manual_request(
            &workspace_id,
            &version_id,
            "mock",
            executable_graph(),
            false,
        ))
        .await
        .expect("first run");
    let first_count = provider.invoke_count();
    let mut graph = executable_graph();
    graph.nodes.get_mut("video").expect("video").params = json!({
        "prompt": "launch teaser",
        "duration_sec": 6,
        "aspect_ratio": "9:16"
    });

    let outcome = service
        .execute_manual_run(manual_request(
            &workspace_id,
            &version_id,
            "mock",
            graph,
            false,
        ))
        .await
        .expect("terminal dirty run");

    assert_eq!(first_count, 2);
    assert_eq!(provider.invoke_count(), first_count + 1);
    assert!(step_cached(node_step(&outcome, "text")));
    assert!(step_cached(node_step(&outcome, "writer")));
    assert!(!step_cached(node_step(&outcome, "video")));
}

#[tokio::test]
async fn node_cache_reruns_middle_node_and_downstream() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let provider = CountingProvider::default();
    let service = RunService::with_provider(store, provider.clone());

    service
        .execute_manual_run(manual_request(
            &workspace_id,
            &version_id,
            "mock",
            executable_graph(),
            false,
        ))
        .await
        .expect("first run");
    let first_count = provider.invoke_count();
    let mut graph = executable_graph();
    graph.nodes.get_mut("writer").expect("writer").params = json!({ "style": "product" });

    let outcome = service
        .execute_manual_run(manual_request(
            &workspace_id,
            &version_id,
            "mock",
            graph,
            false,
        ))
        .await
        .expect("middle dirty run");

    assert_eq!(provider.invoke_count(), first_count + 2);
    assert!(step_cached(node_step(&outcome, "text")));
    assert!(!step_cached(node_step(&outcome, "writer")));
    assert!(!step_cached(node_step(&outcome, "video")));
}

#[tokio::test]
async fn node_cache_treats_corrupt_artifact_file_as_miss() {
    let (store, _dir) = open_temp_store().await;
    let artifact_dir = tempfile::tempdir().expect("artifact dir");
    let (workspace_id, version_id) = workspace_version(&store).await;
    let provider = CountingProvider::default();
    let service = RunService::with_provider_events_and_artifact_root(
        store,
        provider.clone(),
        EventBus::default(),
        artifact_dir.path(),
    );

    let first = service
        .execute_manual_run(manual_request(
            &workspace_id,
            &version_id,
            "mock",
            executable_graph(),
            false,
        ))
        .await
        .expect("first run");
    let writer_artifact = first
        .artifacts
        .iter()
        .find(|artifact| artifact.node_id.as_deref() == Some("writer"))
        .expect("writer artifact");
    tokio::fs::write(
        artifact_dir.path().join(&writer_artifact.storage_uri),
        b"corrupt",
    )
    .await
    .expect("corrupt cached file");
    let first_count = provider.invoke_count();

    let second = service
        .execute_manual_run(manual_request(
            &workspace_id,
            &version_id,
            "mock",
            executable_graph(),
            false,
        ))
        .await
        .expect("second run");

    assert_eq!(provider.invoke_count(), first_count + 2);
    assert!(step_cached(node_step(&second, "text")));
    assert!(!step_cached(node_step(&second, "writer")));
    assert!(!step_cached(node_step(&second, "video")));
}

#[tokio::test]
async fn node_cache_force_rerun_bypasses_existing_entries() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let provider = CountingProvider::default();
    let service = RunService::with_provider(store, provider.clone());

    service
        .execute_manual_run(manual_request(
            &workspace_id,
            &version_id,
            "mock",
            executable_graph(),
            false,
        ))
        .await
        .expect("first run");
    let first_count = provider.invoke_count();
    let forced = service
        .execute_manual_run(manual_request(
            &workspace_id,
            &version_id,
            "mock",
            executable_graph(),
            true,
        ))
        .await
        .expect("forced run");

    assert_eq!(provider.invoke_count(), first_count + 2);
    assert!(forced.steps.iter().all(|step| !step_cached(step)));
}

#[tokio::test]
async fn node_cache_separates_provider_ids() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let provider = CountingProvider::default();
    let service = RunService::with_provider(store, provider.clone());

    service
        .execute_manual_run(manual_request(
            &workspace_id,
            &version_id,
            "mock",
            executable_graph(),
            false,
        ))
        .await
        .expect("mock run");
    let mock_count = provider.invoke_count();
    let alt = service
        .execute_manual_run(manual_request(
            &workspace_id,
            &version_id,
            "alt",
            executable_graph(),
            false,
        ))
        .await
        .expect("alt run");

    assert_eq!(mock_count, 2);
    assert_eq!(provider.invoke_count(), mock_count + 2);
    assert!(!step_cached(node_step(&alt, "writer")));
    assert!(!step_cached(node_step(&alt, "video")));
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
        self.inner
            .invoke(ProviderRequest {
                provider: "mock".to_owned(),
                ..req
            })
            .await
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        self.inner.cancel(handle).await
    }
}
