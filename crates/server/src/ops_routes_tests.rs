use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::to_bytes;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use helixflow_graph::{GraphEdge, GraphNode, WorkflowGraph};
use helixflow_run::EventBus;
use helixflow_store::{NewProposal, NewVersion, Store, VersionSource};
use serde_json::{Value, json};

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::write_json_file;
use crate::ops_routes::apply_workspace_ops;
use crate::test_support::FailingWorkbenchAgent;

#[tokio::test]
async fn ops_route_applies_multiple_ops_as_one_manual_version() {
    let (state, workspace_id, base_version_id, _dir) = state_with_graph().await;
    let body = json!({
        "baseVersionId": base_version_id,
        "ops": [
            { "op": "set_param", "id": "text", "key": "text", "value": "updated" },
            { "op": "move_node", "id": "writer", "pos": [420, 80] },
            { "op": "add_node", "id": "alt", "node_type": "input.text", "title": null, "params": { "text": "alt" }, "pos": [120, 0] },
            { "op": "remove_edge", "from": ["text", "text"], "to": ["writer", "text"], "edge_type": "text" },
            { "op": "add_edge", "from": ["alt", "text"], "to": ["writer", "text"], "edge_type": "text" },
            { "op": "remove_node", "id": "text" }
        ]
    });

    let response = apply_workspace_ops(
        Path(workspace_id.clone()),
        State(state.clone()),
        body.to_string(),
    )
    .await
    .expect("ops response")
    .0;

    let next_version_id = response["workspace"]["versionId"]
        .as_str()
        .expect("version id");
    assert_ne!(next_version_id, base_version_id);
    let version = state.store.version(next_version_id).await.expect("version");
    assert_eq!(version.source, "manual");
    assert_eq!(version.parent_id.as_deref(), Some(base_version_id.as_str()));
    assert!(
        state
            .store
            .latest_pending_proposal(&workspace_id)
            .await
            .expect("pending proposal")
            .is_none()
    );
    assert_eq!(
        response["graph"]["nodes"].as_array().expect("nodes").len(),
        2
    );
    assert!(response["graph"]["nodes"].to_string().contains("alt"));
}

#[tokio::test]
async fn ops_route_rejects_op_level_failures_with_op_index() {
    let (state, workspace_id, base_version_id, _dir) = state_with_graph().await;
    let body = json!({
        "baseVersionId": base_version_id,
        "ops": [
            { "op": "remove_node", "id": "text" },
            { "op": "set_param", "id": "text", "key": "text", "value": "updated" }
        ]
    });

    let err = apply_workspace_ops(
        Path(workspace_id.clone()),
        State(state.clone()),
        body.to_string(),
    )
    .await
    .expect_err("invalid ops should fail");
    let body = error_body(err).await;

    assert_eq!(body["opIndex"], 1);
    assert_eq!(
        state
            .store
            .workspace(&workspace_id)
            .await
            .expect("workspace")
            .cur_version_id
            .as_deref(),
        Some(base_version_id.as_str())
    );
}

#[tokio::test]
async fn ops_route_rejects_graph_level_failures_with_null_op_index() {
    let (state, workspace_id, base_version_id, _dir) = state_with_graph().await;
    let body = json!({
        "baseVersionId": base_version_id,
        "ops": [
            { "op": "add_node", "id": "alt", "node_type": "input.text", "title": null, "params": { "text": "alt" }, "pos": [120, 0] },
            { "op": "add_edge", "from": ["alt", "text"], "to": ["writer", "text"], "edge_type": "text" }
        ]
    });

    let err = apply_workspace_ops(Path(workspace_id), State(state), body.to_string())
        .await
        .expect_err("graph-level invalid ops should fail");
    let body = error_body(err).await;

    assert!(body.get("opIndex").is_some_and(Value::is_null));
}

#[tokio::test]
async fn ops_route_rejects_pending_proposal_and_stale_base_without_writes() {
    let (state, workspace_id, base_version_id, _dir) = state_with_graph().await;
    let graph_count = graph_file_count(&state, &workspace_id).await;
    create_pending_proposal(&state, &workspace_id, &base_version_id).await;
    let body = json!({
        "baseVersionId": base_version_id,
        "ops": [{ "op": "move_node", "id": "writer", "pos": [420, 80] }]
    });

    let err = apply_workspace_ops(
        Path(workspace_id.clone()),
        State(state.clone()),
        body.to_string(),
    )
    .await
    .expect_err("pending proposal should block ops");

    assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
    assert_eq!(graph_file_count(&state, &workspace_id).await, graph_count);
    assert_eq!(
        state
            .store
            .workspace(&workspace_id)
            .await
            .expect("workspace")
            .cur_version_id
            .as_deref(),
        Some(base_version_id.as_str())
    );
}

#[tokio::test]
async fn ops_route_rejects_unknown_fields_with_structured_error() {
    let (state, workspace_id, base_version_id, _dir) = state_with_graph().await;
    let body = json!({
        "baseVersionId": base_version_id,
        "ops": [{ "op": "move_node", "id": "writer", "pos": [420, 80] }],
        "unexpected": true
    });

    let err = apply_workspace_ops(Path(workspace_id), State(state), body.to_string())
        .await
        .expect_err("unknown fields should fail");
    let body = error_body(err).await;

    assert!(
        body["error"]
            .as_str()
            .expect("error")
            .contains("unknown field")
    );
}

async fn state_with_graph() -> (AppState, String, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let data_dir = dir.path().to_path_buf();
    let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    let workspace = store
        .create_workspace("Ops workspace")
        .await
        .expect("create workspace");
    let graph_path = std::path::PathBuf::from("workspaces")
        .join(&workspace.id)
        .join("graphs")
        .join("initial.json");
    let graph_hash = write_json_file(&data_dir, &graph_path, &ops_graph(), "write graph")
        .await
        .expect("write graph");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Initial graph",
            source: VersionSource::Manual,
            graph_path: graph_path.to_string_lossy().as_ref(),
            graph_hash: &graph_hash,
            parent_id: None,
        })
        .await
        .expect("create version");
    let state = AppState::with_store_agent(
        EventBus::new(16),
        store,
        data_dir.clone(),
        Arc::new(FailingWorkbenchAgent),
        data_dir.join("sessions"),
    );
    (state, workspace.id, version.id, dir)
}

fn ops_graph() -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([
            (
                "text".to_owned(),
                GraphNode {
                    node_type: "input.text".to_owned(),
                    title: "Text".to_owned(),
                    params: json!({ "text": "old" }),
                    pos: [0.0, 0.0],
                },
            ),
            (
                "writer".to_owned(),
                GraphNode {
                    node_type: "llm.prompt_writer".to_owned(),
                    title: "Writer".to_owned(),
                    params: json!({ "style": "plain" }),
                    pos: [240.0, 0.0],
                },
            ),
        ]),
        edges: vec![GraphEdge {
            from: ["text".to_owned(), "text".to_owned()],
            to: ["writer".to_owned(), "text".to_owned()],
            edge_type: "text".to_owned(),
        }],
    }
}

async fn create_pending_proposal(state: &AppState, workspace_id: &str, base_version_id: &str) {
    state
        .store
        .create_proposal(NewProposal {
            workspace_id,
            base_version_id,
            kind: "modify",
            title: "Pending",
            summary: "Pending proposal",
            ops_path: "pending/ops.json",
            preview_graph_path: None,
            message_id: None,
        })
        .await
        .expect("create proposal");
}

async fn graph_file_count(state: &AppState, workspace_id: &str) -> usize {
    let graph_dir = state
        .data_dir
        .join("workspaces")
        .join(workspace_id)
        .join("graphs");
    let mut entries = tokio::fs::read_dir(graph_dir).await.expect("graph dir");
    let mut count = 0;
    while let Some(entry) = entries.next_entry().await.expect("graph entry") {
        if entry.path().extension().is_some_and(|ext| ext == "json") {
            count += 1;
        }
    }
    count
}

async fn error_body(err: ApiError) -> Value {
    let response = err.into_response();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("error body");
    serde_json::from_slice(&bytes).expect("json body")
}
