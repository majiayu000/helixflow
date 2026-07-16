use std::sync::Arc;

use axum::Json;
use axum::body::to_bytes;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use futures_util::future::join_all;
use helixflow_run::EventBus;
use helixflow_store::Store;
use serde_json::json;

use super::*;
use crate::graph_files::write_json_file;
use crate::test_support::FailingWorkbenchAgent;

#[tokio::test]
async fn comment_ops_persist_and_reload_from_canvas_snapshot() {
    let (_dir, state, workspace_id) = comment_test_state().await;

    let added = apply_canvas_comment_op(
        AxumPath(workspace_id.clone()),
        State(state.clone()),
        Json(CanvasCommentOpRequest {
            base_seq: None,
            operation_id: Some("operation_add".to_owned()),
            op: CanvasCommentOp::CommentAdd {
                id: Some("comment_1".to_owned()),
                target: CanvasCommentTarget::Position { x: 20.0, y: 30.0 },
                body: " Needs review ".to_owned(),
                actor: Some(CanvasActor {
                    actor_id: "reviewer".to_owned(),
                    display_name: "Reviewer".to_owned(),
                }),
            },
        }),
    )
    .await
    .expect("add comment")
    .0;

    assert_eq!(added["comments"][0]["id"], "comment_1");
    assert_eq!(added["comments"][0]["body"], "Needs review");
    assert_eq!(added["comments"][0]["status"], "open");

    let resolved = apply_canvas_comment_op(
        AxumPath(workspace_id.clone()),
        State(state.clone()),
        Json(CanvasCommentOpRequest {
            base_seq: None,
            operation_id: Some("operation_resolve".to_owned()),
            op: CanvasCommentOp::CommentPatch {
                id: "comment_1".to_owned(),
                body: None,
                status: Some(CanvasCommentStatus::Resolved),
            },
        }),
    )
    .await
    .expect("resolve comment")
    .0;
    assert_eq!(resolved["comments"][0]["status"], "resolved");

    let reloaded = workspace_canvas_value(&state, &workspace_id)
        .await
        .expect("reload canvas");
    assert_eq!(reloaded["comments"][0]["id"], "comment_1");
    assert_eq!(reloaded["comments"][0]["status"], "resolved");

    let deleted = apply_canvas_comment_op(
        AxumPath(workspace_id.clone()),
        State(state.clone()),
        Json(CanvasCommentOpRequest {
            base_seq: None,
            operation_id: Some("operation_delete".to_owned()),
            op: CanvasCommentOp::CommentDelete {
                id: "comment_1".to_owned(),
            },
        }),
    )
    .await
    .expect("delete comment")
    .0;
    assert_eq!(deleted["comments"].as_array().expect("comments").len(), 0);
}

#[tokio::test]
async fn stale_base_returns_409_with_current_seq_without_partial_write() {
    let (_dir, state, workspace_id) = comment_test_state().await;
    let _response = submit_add(&state, &workspace_id, 0, 0)
        .await
        .expect("first operation");

    let error = submit_add(&state, &workspace_id, 1, 0)
        .await
        .expect_err("stale operation must conflict");
    assert_eq!(error.status, StatusCode::CONFLICT);
    let response = error.into_response();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read conflict response");
    let body: serde_json::Value = serde_json::from_slice(&bytes).expect("parse conflict response");
    assert_eq!(body["currentSeq"], 1);

    let state = read_comment_store(&state.store, &workspace_id)
        .await
        .expect("read comments");
    assert_eq!(state.seq, 1);
    assert_eq!(state.comments.len(), 1);
    assert!(
        state
            .comments
            .iter()
            .all(|comment| comment.body != "Concurrent comment 1")
    );
}

#[tokio::test]
async fn stable_operation_id_replay_does_not_duplicate_or_increment_seq() {
    let (_dir, state, workspace_id) = comment_test_state().await;
    let _first_response = submit_add(&state, &workspace_id, 0, 0)
        .await
        .expect("first operation");
    let _replay_response = submit_add(&state, &workspace_id, 0, 0)
        .await
        .expect("idempotent replay");

    let snapshot = read_comment_store(&state.store, &workspace_id)
        .await
        .expect("read comments");
    assert_eq!(snapshot.seq, 1);
    assert_eq!(snapshot.comments.len(), 1);
}

#[tokio::test]
async fn operation_id_reuse_with_different_content_returns_409() {
    let (_dir, state, workspace_id) = comment_test_state().await;
    let _response = submit_add(&state, &workspace_id, 0, 0)
        .await
        .expect("first operation");
    let error = apply_canvas_comment_op(
        AxumPath(workspace_id.clone()),
        State(state.clone()),
        Json(CanvasCommentOpRequest {
            base_seq: Some(1),
            operation_id: Some("operation_0".to_owned()),
            op: CanvasCommentOp::CommentDelete {
                id: "comment_0".to_owned(),
            },
        }),
    )
    .await
    .expect_err("operation id reuse must conflict");
    assert_eq!(error.status, StatusCode::CONFLICT);

    let snapshot = read_comment_store(&state.store, &workspace_id)
        .await
        .expect("read comments");
    assert_eq!(snapshot.seq, 1);
    assert_eq!(snapshot.comments.len(), 1);
}

#[tokio::test]
async fn concurrent_same_base_comment_ops_retry_to_forty_without_loss() {
    let (_dir, state, workspace_id) = comment_test_state().await;
    let requests = (0..40).map(|index| {
        let state = state.clone();
        let workspace_id = workspace_id.clone();
        async move { (index, submit_add(&state, &workspace_id, index, 0).await) }
    });

    let results = join_all(requests).await;
    let accepted = results.iter().filter(|(_, result)| result.is_ok()).count();
    let conflicts = results
        .iter()
        .filter(|(_, result)| {
            result
                .as_ref()
                .is_err_and(|error| error.status == StatusCode::CONFLICT)
        })
        .count();
    assert_eq!(accepted, 1);
    assert_eq!(conflicts, 39);

    for index in 0..40 {
        let current_seq = read_comment_store(&state.store, &workspace_id)
            .await
            .expect("refresh sequence")
            .seq;
        let _response = submit_add(&state, &workspace_id, index, current_seq)
            .await
            .expect("retry operation after refresh");
    }

    let snapshot = read_comment_store(&state.store, &workspace_id)
        .await
        .expect("read final comments");
    eprintln!(
        "same-base green proof: submitted=40 initialAccepted={accepted} conflicts={conflicts} persisted={} seq={}",
        snapshot.comments.len(),
        snapshot.seq
    );
    assert_eq!(snapshot.comments.len(), 40);
    assert_eq!(snapshot.seq, 40);
}

#[tokio::test]
async fn legacy_json_imports_once_and_never_becomes_truth_again() {
    let (_dir, state, workspace_id) = comment_test_state().await;
    let relative = comments_relative_path(&workspace_id).expect("comments path");
    write_json_file(
        &state.data_dir,
        &relative,
        &json!({
            "schemaVersion": 1,
            "seq": 1,
            "comments": [comment_json("legacy_comment")]
        }),
        "write legacy comments",
    )
    .await
    .expect("write legacy comments");

    let imported = load_canvas_comments(&state.data_dir, &workspace_id)
        .await
        .expect("import legacy comments");
    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0].id, "legacy_comment");
    assert_eq!(
        state
            .store
            .canvas_comment_state(&workspace_id)
            .await
            .expect("read imported state")
            .expect("state exists")
            .migration_source,
        "legacy_json"
    );

    tokio::fs::write(state.data_dir.join(relative), b"not valid json")
        .await
        .expect("corrupt old legacy file");
    let reloaded = load_canvas_comments(&state.data_dir, &workspace_id)
        .await
        .expect("reload SQLite comments");
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].id, "legacy_comment");
}

#[tokio::test]
async fn invalid_legacy_json_fails_closed_without_initializing_empty_state() {
    let (_dir, state, workspace_id) = comment_test_state().await;
    let relative = comments_relative_path(&workspace_id).expect("comments path");
    tokio::fs::create_dir_all(
        state
            .data_dir
            .join(relative.parent().expect("comments parent")),
    )
    .await
    .expect("create legacy directory");
    tokio::fs::write(state.data_dir.join(relative), b"not valid json")
        .await
        .expect("write invalid legacy file");

    let error = load_canvas_comments(&state.data_dir, &workspace_id)
        .await
        .expect_err("invalid legacy JSON must fail");
    assert!(error.message.contains("contains invalid JSON"));
    assert!(
        state
            .store
            .canvas_comment_state(&workspace_id)
            .await
            .expect("read comment state")
            .is_none()
    );
}

#[tokio::test]
async fn presence_broadcasts_without_creating_durable_comments() {
    let (_dir, state, workspace_id) = comment_test_state().await;
    let mut receiver = state.events.subscribe();

    let body = update_canvas_presence(
        AxumPath(workspace_id.clone()),
        State(state.clone()),
        Json(CanvasPresencePayload {
            actor: CanvasActor {
                actor_id: "actor_remote".to_owned(),
                display_name: "Remote".to_owned(),
            },
            cursor: Some(CanvasPoint { x: 42.0, y: 55.0 }),
            selection: Some(CanvasPresenceSelection {
                node_ids: vec!["node_a".to_owned()],
                edge_ids: Vec::new(),
            }),
            viewport: Some(CanvasViewport {
                x: 1.0,
                y: 2.0,
                zoom: 1.1,
            }),
        }),
    )
    .await
    .expect("presence")
    .0;

    assert_eq!(body["ok"], true);
    let event = receiver.recv().await.expect("presence event");
    assert_eq!(event.workspace_id, workspace_id);
    assert_eq!(event.ev, "canvas.presence");
    assert_eq!(event.data["actor"]["actorId"], "actor_remote");
    assert!(
        read_comment_store(&state.store, &workspace_id)
            .await
            .is_err(),
        "presence must not initialize durable comment state"
    );
}

async fn submit_add(
    state: &AppState,
    workspace_id: &str,
    index: usize,
    base_seq: i64,
) -> Result<Json<Value>, ApiError> {
    apply_canvas_comment_op(
        AxumPath(workspace_id.to_owned()),
        State(state.clone()),
        Json(CanvasCommentOpRequest {
            base_seq: Some(base_seq),
            operation_id: Some(format!("operation_{index}")),
            op: CanvasCommentOp::CommentAdd {
                id: Some(format!("comment_{index}")),
                target: CanvasCommentTarget::Position {
                    x: index as f32,
                    y: 0.0,
                },
                body: format!("Concurrent comment {index}"),
                actor: None,
            },
        }),
    )
    .await
}

fn comment_json(id: &str) -> serde_json::Value {
    json!({
        "id": id,
        "target": { "kind": "position", "x": 1.0, "y": 2.0 },
        "body": "Legacy",
        "author": { "actorId": "legacy", "displayName": "Legacy" },
        "status": "open",
        "createdAt": "unix:1",
        "updatedAt": "unix:1"
    })
}

async fn comment_test_state() -> (tempfile::TempDir, AppState, String) {
    let dir = tempfile::tempdir().expect("temp dir");
    let data_dir = dir.path().to_path_buf();
    let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    let workspace = store
        .create_workspace("Canvas collaboration")
        .await
        .expect("workspace");
    let state = AppState::with_store_agent(
        EventBus::new(16),
        store,
        data_dir.clone(),
        Arc::new(FailingWorkbenchAgent),
        data_dir.join("sessions"),
    );
    (dir, state, workspace.id)
}
