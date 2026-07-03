use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use helixflow_gateway::RuntimeProvider;
use helixflow_graph::{GraphEdge, GraphNode, WorkflowGraph};
use helixflow_run::EventBus;
use helixflow_store::{NewVersion, Store, VersionSource};
use serde_json::json;

use crate::app_state::AppState;
use crate::run_routes::queue_workspace_run;
use crate::test_support::FailingWorkbenchAgent;

#[tokio::test]
async fn queue_route_with_unavailable_fal_provider_fails_without_mock_outputs() {
    let (state, workspace_id, _version_id, _dir) = state_with_workspace_provider(
        RuntimeProvider::unavailable("fal", "FAL_KEY is not configured"),
    )
    .await;
    write_graph(&state, &executable_graph()).await;

    let err = queue_workspace_run(Path(workspace_id.clone()), State(state.clone()), None)
        .await
        .expect_err("unavailable provider should reject route run");

    assert_eq!(err.status, StatusCode::CONFLICT);
    assert!(err.message.contains("fal"));
    let run = state
        .store
        .latest_workspace_run(&workspace_id)
        .await
        .expect("workspace run lookup");
    assert!(run.is_none());
}

async fn state_with_workspace_provider(
    provider: RuntimeProvider,
) -> (AppState, String, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let data_dir = dir.path().to_path_buf();
    let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    let workspace = store
        .create_workspace("Run route workspace")
        .await
        .expect("create workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Run graph",
            source: VersionSource::Manual,
            graph_path: "graphs/run.json",
            graph_hash: "sha256:run",
            parent_id: None,
        })
        .await
        .expect("create version");
    let state = AppState::with_store_agent_provider(
        EventBus::new(16),
        store,
        data_dir.clone(),
        Arc::new(FailingWorkbenchAgent),
        data_dir.join("sessions"),
        provider,
    );
    (state, workspace.id, version.id, dir)
}

async fn write_graph(state: &AppState, graph: &WorkflowGraph) {
    let graph_path = state.data_dir.join("graphs/run.json");
    tokio::fs::create_dir_all(graph_path.parent().expect("graph parent"))
        .await
        .expect("create graph dir");
    tokio::fs::write(
        graph_path,
        serde_json::to_vec_pretty(graph).expect("encode graph"),
    )
    .await
    .expect("write graph");
}

fn executable_graph() -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([
            (
                "image".to_owned(),
                GraphNode {
                    node_type: "image.generate".to_owned(),
                    title: "Image render".to_owned(),
                    params: json!({
                        "prompt": "clean product shot",
                        "aspect_ratio": "1:1"
                    }),
                    pos: [0.0, 0.0],
                    size: None,
                },
            ),
            (
                "save".to_owned(),
                GraphNode {
                    node_type: "output.save".to_owned(),
                    title: "Save".to_owned(),
                    params: json!({}),
                    pos: [240.0, 0.0],
                    size: None,
                },
            ),
        ]),
        edges: vec![GraphEdge {
            from: ["image".to_owned(), "image".to_owned()],
            to: ["save".to_owned(), "artifact".to_owned()],
            edge_type: "artifact".to_owned(),
        }],
    }
}
