use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::to_bytes;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use helixflow_graph::{GraphEdge, GraphNode, WorkflowGraph};
use helixflow_run::EventBus;
use helixflow_store::{NewProposal, NewVersion, Store, VersionSource};
use serde_json::{Value, json};
use tokio::sync::Barrier;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::{graph_hash, write_json_file};
use crate::ops_routes::{apply_workspace_ops, ops_idempotency_digest};
use crate::test_support::FailingWorkbenchAgent;
use crate::version_file_consistency::{VersionFileCandidate, read_version_graph};

#[tokio::test]
async fn ops_route_applies_multiple_ops_as_one_manual_version() {
    let (state, workspace_id, base_version_id, _dir) = state_with_graph().await;
    let body = json!({
        "baseVersionId": base_version_id,
        "ops": [
            { "op": "set_param", "id": "text", "key": "text", "value": "updated" },
            { "op": "move_node", "id": "writer", "pos": [420, 80] },
            { "op": "resize_node", "id": "writer", "size": [260, 180] },
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
    assert_eq!(
        response["workflowGraph"]["nodes"]["writer"]["size"][0],
        260.0
    );
    assert_eq!(
        response["workflowGraph"]["nodes"]["writer"]["size"][1],
        180.0
    );
}

#[tokio::test]
async fn idempotency_reuses_exact_key_and_payload_without_duplicate_versions() {
    let (state, workspace_id, base_version_id, _dir) = state_with_graph().await;
    let body = json!({
        "baseVersionId": base_version_id,
        "idempotencyKey": "canvas_op_retry_1",
        "ops": [{ "op": "move_node", "id": "writer", "pos": [420, 80] }]
    });

    let first = apply_workspace_ops(
        Path(workspace_id.clone()),
        State(state.clone()),
        body.to_string(),
    )
    .await
    .expect("first ops response")
    .0;
    let first_version_id = first["workspace"]["versionId"]
        .as_str()
        .expect("first version id")
        .to_owned();
    let graph_count = graph_file_count(&state, &workspace_id).await;
    let version_count = state
        .store
        .versions_for_workspace(&workspace_id)
        .await
        .expect("versions")
        .len();

    let retry = apply_workspace_ops(
        Path(workspace_id.clone()),
        State(state.clone()),
        body.to_string(),
    )
    .await
    .expect("retry ops response")
    .0;

    assert_eq!(retry["workspace"]["versionId"], first_version_id);
    assert_eq!(graph_file_count(&state, &workspace_id).await, graph_count);
    assert_eq!(
        state
            .store
            .versions_for_workspace(&workspace_id)
            .await
            .expect("versions")
            .len(),
        version_count
    );
}

#[tokio::test]
async fn idempotency_same_key_different_payload_conflicts_without_mutating_winner() {
    let (state, workspace_id, base_version_id, _dir) = state_with_graph().await;
    let key = "same-key-different-payload";
    let first_body = ops_move_body(&base_version_id, Some(key), 420.0);
    apply_ops_json(&state, &workspace_id, &first_body)
        .await
        .expect("first keyed ops");
    let winner_id = state
        .store
        .workspace(&workspace_id)
        .await
        .expect("workspace after first")
        .cur_version_id
        .expect("winner id");
    let winner = state.store.version(&winner_id).await.expect("winner");
    let winner_bytes = tokio::fs::read(state.data_dir.join(&winner.graph_path))
        .await
        .expect("winner bytes");

    let error = apply_ops_json(
        &state,
        &workspace_id,
        &ops_move_body(&base_version_id, Some(key), 520.0),
    )
    .await
    .expect_err("same key with different canonical payload must conflict");

    assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
    assert_eq!(
        state
            .store
            .workspace(&workspace_id)
            .await
            .expect("workspace after collision")
            .cur_version_id,
        Some(winner_id)
    );
    assert_eq!(
        tokio::fs::read(state.data_dir.join(&winner.graph_path))
            .await
            .expect("winner bytes after collision"),
        winner_bytes
    );
    assert_eq!(
        state
            .store
            .versions_for_workspace(&workspace_id)
            .await
            .expect("versions after collision")
            .len(),
        2
    );
}

#[tokio::test]
async fn idempotency_different_keys_with_same_content_do_not_alias() {
    let (state, workspace_id, base_version_id, _dir) = state_with_graph().await;
    let first_path = keyed_graph_path(&workspace_id, &base_version_id, "first-key");
    let second_path = keyed_graph_path(&workspace_id, &base_version_id, "second-key");
    assert_ne!(first_path, second_path);
    let first_body = ops_move_body(&base_version_id, Some("first-key"), 420.0);
    apply_ops_json(&state, &workspace_id, &first_body)
        .await
        .expect("first keyed ops");

    let error = apply_ops_json(
        &state,
        &workspace_id,
        &ops_move_body(&base_version_id, Some("second-key"), 420.0),
    )
    .await
    .expect_err("different key remains a distinct same-base write");

    assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
    assert!(state.data_dir.join(first_path).exists());
    assert!(
        !state.data_dir.join(second_path).exists(),
        "losing distinct candidate must be cleaned"
    );
}

#[tokio::test]
async fn idempotency_existing_file_without_reference_conflicts_without_delete_or_key_leak() {
    let (state, workspace_id, base_version_id, _dir) = state_with_graph().await;
    let raw_key = "secret/raw key?token=never-expose";
    let path = keyed_graph_path(&workspace_id, &base_version_id, raw_key);
    assert!(!path.to_string_lossy().contains(raw_key));
    tokio::fs::write(state.data_dir.join(&path), b"unowned-canary")
        .await
        .expect("write unowned collision");

    let error = apply_ops_json(
        &state,
        &workspace_id,
        &ops_move_body(&base_version_id, Some(raw_key), 420.0),
    )
    .await
    .expect_err("file without exact DB reference cannot replay");

    assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
    assert!(!error.message.contains(raw_key));
    assert_eq!(
        tokio::fs::read(state.data_dir.join(path))
            .await
            .expect("unowned collision preserved"),
        b"unowned-canary"
    );
    assert_eq!(
        state
            .store
            .versions_for_workspace(&workspace_id)
            .await
            .expect("versions after unowned collision")
            .len(),
        1
    );
}

#[tokio::test]
async fn idempotency_mismatched_single_reference_and_multiple_references_both_conflict() {
    assert_non_exact_reference_collision(false).await;
    assert_non_exact_reference_collision(true).await;
}

async fn assert_non_exact_reference_collision(multiple_references: bool) {
    let (state, workspace_id, base_version_id, _dir) = state_with_graph().await;
    let key = if multiple_references {
        "multiple-references"
    } else {
        "mismatched-reference"
    };
    let mut edited_graph = ops_graph();
    edited_graph.nodes.get_mut("writer").expect("writer").pos = [420.0, 80.0];
    let digest = ops_idempotency_digest(&workspace_id, &base_version_id, key);
    let mut candidate =
        VersionFileCandidate::from_keyed_ops_graph(&workspace_id, &edited_graph, &digest)
            .expect("keyed candidate");
    let graph_path = candidate
        .relative_path_text()
        .expect("candidate path")
        .to_owned();
    let candidate_hash = candidate.graph_hash().to_owned();
    candidate
        .publish(&state.data_dir)
        .expect("publish collision fixture");
    let reference_count = if multiple_references { 2 } else { 1 };
    for index in 0..reference_count {
        let reference_workspace_id = if multiple_references {
            state
                .store
                .create_workspace(&format!("Reference {index}"))
                .await
                .expect("create reference workspace")
                .id
        } else {
            workspace_id.clone()
        };
        state
            .store
            .create_version(NewVersion {
                workspace_id: &reference_workspace_id,
                label: "Non-exact reference",
                source: VersionSource::Manual,
                graph_path: &graph_path,
                graph_hash: &candidate_hash,
                parent_id: None,
            })
            .await
            .expect("create non-exact reference");
    }
    candidate.mark_committed().expect("mark fixture committed");
    let before_bytes = tokio::fs::read(state.data_dir.join(&graph_path))
        .await
        .expect("fixture bytes");
    let target_version_count = state
        .store
        .versions_for_workspace(&workspace_id)
        .await
        .expect("target versions before collision")
        .len();

    let error = apply_ops_json(
        &state,
        &workspace_id,
        &ops_move_body(&base_version_id, Some(key), 420.0),
    )
    .await
    .expect_err("non-exact reference shape must conflict");

    assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
    assert_eq!(
        tokio::fs::read(state.data_dir.join(&graph_path))
            .await
            .expect("fixture bytes after collision"),
        before_bytes
    );
    assert_eq!(
        state
            .store
            .versions_for_workspace(&workspace_id)
            .await
            .expect("target versions after collision")
            .len(),
        target_version_count
    );
}

#[test]
fn idempotency_digest_is_domain_separated_length_delimited_and_strictly_opaque() {
    let first = ops_idempotency_digest("ab", "c", "d");
    let second = ops_idempotency_digest("a", "bc", "d");
    assert_ne!(first, second);
    assert_eq!(first.len(), 64);
    assert!(
        first
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
    assert!(VersionFileCandidate::from_keyed_ops_graph("ws_test", &ops_graph(), &first).is_ok());
    assert!(VersionFileCandidate::from_keyed_ops_graph("ws_test", &ops_graph(), "ABC").is_err());
}

#[tokio::test]
async fn concurrent_same_base_ops_has_one_winner_and_cleans_loser_candidate() {
    let (state, workspace_id, base_version_id, _dir) = state_with_graph().await;
    let barrier = Arc::new(Barrier::new(3));
    let first = spawn_ops(
        state.clone(),
        Arc::clone(&barrier),
        workspace_id.clone(),
        ops_move_body(&base_version_id, None, 420.0),
    );
    let second = spawn_ops(
        state.clone(),
        Arc::clone(&barrier),
        workspace_id.clone(),
        ops_move_body(&base_version_id, None, 520.0),
    );
    barrier.wait().await;
    let results = [
        first.await.expect("first ops task"),
        second.await.expect("second ops task"),
    ];

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter_map(|result| result.as_ref().err())
            .filter(|error| error.status == axum::http::StatusCode::CONFLICT)
            .count(),
        1,
        "results: {results:?}"
    );
    let winner_id = state
        .store
        .workspace(&workspace_id)
        .await
        .expect("workspace after race")
        .cur_version_id
        .expect("winner id");
    let winner = state.store.version(&winner_id).await.expect("winner");
    let bytes = tokio::fs::read(state.data_dir.join(&winner.graph_path))
        .await
        .expect("winner bytes");
    let graph = read_version_graph(&state.data_dir, &winner)
        .await
        .expect("verified winner");
    assert_eq!(winner.parent_id.as_deref(), Some(base_version_id.as_str()));
    assert_eq!(winner.graph_hash, graph_hash(&bytes));
    assert!(matches!(graph.nodes["writer"].pos[0], 420.0 | 520.0));
    assert_eq!(
        state
            .store
            .versions_for_workspace(&workspace_id)
            .await
            .expect("versions after race")
            .len(),
        2
    );
    assert_eq!(graph_file_count(&state, &workspace_id).await, 2);
}

#[tokio::test]
async fn idempotency_replay_after_later_advance_returns_latest_state_without_duplicate() {
    let (state, workspace_id, base_version_id, _dir) = state_with_graph().await;
    let original = ops_move_body(&base_version_id, Some("replay-after-advance"), 420.0);
    let first = apply_ops_json(&state, &workspace_id, &original)
        .await
        .expect("original keyed ops");
    let original_version_id = first["workspace"]["versionId"]
        .as_str()
        .expect("original version id")
        .to_owned();
    let advanced = apply_ops_json(
        &state,
        &workspace_id,
        &ops_move_body(&original_version_id, None, 520.0),
    )
    .await
    .expect("later unkeyed advance");
    let latest_version_id = advanced["workspace"]["versionId"]
        .as_str()
        .expect("latest version id")
        .to_owned();
    let version_count = state
        .store
        .versions_for_workspace(&workspace_id)
        .await
        .expect("versions before replay")
        .len();

    let replay = apply_ops_json(&state, &workspace_id, &original)
        .await
        .expect("exact historical replay");

    assert_ne!(latest_version_id, original_version_id);
    assert_eq!(replay["workspace"]["versionId"], latest_version_id);
    assert_eq!(
        state
            .store
            .versions_for_workspace(&workspace_id)
            .await
            .expect("versions after replay")
            .len(),
        version_count
    );
}

fn ops_move_body(base_version_id: &str, key: Option<&str>, x: f32) -> Value {
    let mut body = json!({
        "baseVersionId": base_version_id,
        "ops": [{ "op": "move_node", "id": "writer", "pos": [x, 80] }]
    });
    if let Some(key) = key {
        body["idempotencyKey"] = Value::String(key.to_owned());
    }
    body
}

async fn apply_ops_json(
    state: &AppState,
    workspace_id: &str,
    body: &Value,
) -> Result<Value, ApiError> {
    apply_workspace_ops(
        Path(workspace_id.to_owned()),
        State(state.clone()),
        body.to_string(),
    )
    .await
    .map(|response| response.0)
}

fn keyed_graph_path(workspace_id: &str, base_version_id: &str, key: &str) -> PathBuf {
    PathBuf::from("workspaces")
        .join(workspace_id)
        .join("graphs")
        .join(format!(
            "ops-key-{}.json",
            ops_idempotency_digest(workspace_id, base_version_id, key)
        ))
}

fn spawn_ops(
    state: AppState,
    barrier: Arc<Barrier>,
    workspace_id: String,
    body: Value,
) -> tokio::task::JoinHandle<Result<Value, ApiError>> {
    tokio::spawn(async move {
        barrier.wait().await;
        apply_ops_json(&state, &workspace_id, &body).await
    })
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
async fn ops_route_rejects_set_param_prev_conflict_with_op_index() {
    let (state, workspace_id, base_version_id, _dir) = state_with_graph().await;
    let body = json!({
        "baseVersionId": base_version_id,
        "ops": [
            { "op": "set_param", "id": "text", "key": "text", "prev": "newer", "value": "updated" }
        ]
    });

    let err = apply_workspace_ops(
        Path(workspace_id.clone()),
        State(state.clone()),
        body.to_string(),
    )
    .await
    .expect_err("prev mismatch should fail");
    assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
    let body = error_body(err).await;

    assert_eq!(body["opIndex"], 0);
    assert!(
        body["error"]
            .as_str()
            .expect("error")
            .contains("set_param conflict")
    );
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
async fn ops_route_rejects_invalid_resize_without_writes() {
    let (state, workspace_id, base_version_id, _dir) = state_with_graph().await;
    let graph_count = graph_file_count(&state, &workspace_id).await;
    let body = json!({
        "baseVersionId": base_version_id,
        "ops": [{ "op": "resize_node", "id": "writer", "size": [0, 180] }]
    });

    let err = apply_workspace_ops(
        Path(workspace_id.clone()),
        State(state.clone()),
        body.to_string(),
    )
    .await
    .expect_err("invalid resize should fail");
    let body = error_body(err).await;

    assert_eq!(body["opIndex"], 0);
    assert!(body["error"].as_str().expect("error").contains("node size"));
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
                    size: None,
                },
            ),
            (
                "writer".to_owned(),
                GraphNode {
                    node_type: "llm.prompt_writer".to_owned(),
                    title: "Writer".to_owned(),
                    params: json!({ "style": "plain" }),
                    pos: [240.0, 0.0],
                    size: None,
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
