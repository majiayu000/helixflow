use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_graph::{GraphNode, WorkflowGraph};
use helixflow_run::EventBus;
use helixflow_store::{NewVersion, Store, VersionSource};
use serde_json::json;

use crate::app_state::AppState;
use crate::canvas_collaboration::CanvasViewport;
use crate::graph_files::write_json_file;
use crate::layout_routes::{
    NodePositionUpdate, NodeSizeUpdate, SaveCanvasSnapshotRequest, save_canvas_snapshot,
};
use crate::test_support::FailingWorkbenchAgent;

#[tokio::test]
async fn spatial_updates_advance_canvas_revision_without_creating_version() {
    let (state, workspace_id, version_id, _dir) = state_with_graph().await;

    let body = save_canvas_snapshot(
        Path(workspace_id.clone()),
        State(state.clone()),
        Json(SaveCanvasSnapshotRequest {
            version_id: version_id.clone(),
            base_revision: 0,
            positions: vec![NodePositionUpdate {
                id: "input".to_owned(),
                x: 12.0,
                y: 34.0,
            }],
            sizes: vec![NodeSizeUpdate {
                id: "input".to_owned(),
                width: 260.0,
                height: 180.0,
            }],
            viewport: Some(CanvasViewport {
                x: 5.0,
                y: 6.0,
                zoom: 1.25,
            }),
        }),
    )
    .await
    .expect("save snapshot")
    .0;

    assert_eq!(body["versionId"], version_id);
    assert_eq!(body["revision"], 1);
    assert_eq!(body["nodes"][0]["position"]["x"], 12.0);
    assert_eq!(body["nodes"][0]["size"]["width"], 260.0);
    assert_eq!(body["viewport"]["zoom"], 1.25);
    assert_eq!(
        state
            .store
            .versions_for_workspace(&workspace_id)
            .await
            .expect("versions")
            .len(),
        1
    );
}

#[tokio::test]
async fn stale_canvas_revision_fails_clearly() {
    let (state, workspace_id, version_id, _dir) = state_with_graph().await;
    let request = || SaveCanvasSnapshotRequest {
        version_id: version_id.clone(),
        base_revision: 0,
        positions: vec![NodePositionUpdate {
            id: "input".to_owned(),
            x: 12.0,
            y: 34.0,
        }],
        sizes: vec![],
        viewport: None,
    };
    let _ = save_canvas_snapshot(
        Path(workspace_id.clone()),
        State(state.clone()),
        Json(request()),
    )
    .await
    .expect("first save");

    let error = save_canvas_snapshot(Path(workspace_id), State(state), Json(request()))
        .await
        .expect_err("stale save");

    assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
    assert!(error.message.contains("current revision is `1`"));
}

async fn state_with_graph() -> (AppState, String, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let data_dir = dir.path().to_path_buf();
    let database_url = format!("sqlite://{}", data_dir.join("store.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    let workspace = store.create_workspace("Canvas").await.expect("workspace");
    let graph = WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([(
            "input".to_owned(),
            GraphNode {
                node_type: "input.text".to_owned(),
                title: "Input".to_owned(),
                params: json!({ "text": "hello" }),
                pos: [0.0, 0.0],
                size: None,
                semantics: None,
            },
        )]),
        edges: vec![],
        catalog_revision: None,
    };
    let graph_path = format!("workspaces/{}/graphs/base.json", workspace.id);
    let graph_hash = write_json_file(
        &data_dir,
        std::path::Path::new(&graph_path),
        &graph,
        "write graph",
    )
    .await
    .expect("write graph");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Base",
            source: VersionSource::Manual,
            graph_path: &graph_path,
            graph_hash: &graph_hash,
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("version");
    let state = AppState::with_store_agent(
        EventBus::new(32),
        store,
        data_dir.clone(),
        Arc::new(FailingWorkbenchAgent),
        data_dir.join("sessions"),
    );
    (state, workspace.id, version.id, dir)
}
