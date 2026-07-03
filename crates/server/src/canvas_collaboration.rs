use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path as AxumPath, State},
};
use helixflow_run::RunEventEnvelope;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::write_json_file;
use crate::workspace_canvas::workspace_canvas_value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CanvasComment {
    id: String,
    target: CanvasCommentTarget,
    body: String,
    author: CanvasActor,
    status: CanvasCommentStatus,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum CanvasCommentTarget {
    Node {
        #[serde(rename = "nodeId")]
        node_id: String,
    },
    Edge {
        #[serde(rename = "edgeId")]
        edge_id: String,
    },
    Position {
        x: f32,
        y: f32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CanvasActor {
    actor_id: String,
    display_name: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CanvasCommentStatus {
    Open,
    Resolved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CanvasPresencePayload {
    actor: CanvasActor,
    cursor: Option<CanvasPoint>,
    selection: Option<CanvasPresenceSelection>,
    viewport: Option<CanvasViewport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CanvasPoint {
    x: f32,
    y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CanvasPresenceSelection {
    node_ids: Vec<String>,
    edge_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CanvasViewport {
    x: f32,
    y: f32,
    zoom: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CanvasCommentOpRequest {
    #[allow(dead_code)]
    base_seq: Option<i64>,
    op: CanvasCommentOp,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum CanvasCommentOp {
    CommentAdd {
        id: Option<String>,
        target: CanvasCommentTarget,
        body: String,
        actor: Option<CanvasActor>,
    },
    CommentPatch {
        id: String,
        body: Option<String>,
        status: Option<CanvasCommentStatus>,
    },
    CommentDelete {
        id: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CanvasCommentStore {
    schema_version: u32,
    seq: i64,
    comments: Vec<CanvasComment>,
}

impl Default for CanvasCommentStore {
    fn default() -> Self {
        Self {
            schema_version: 1,
            seq: 0,
            comments: Vec::new(),
        }
    }
}

pub(crate) async fn apply_canvas_comment_op(
    AxumPath(workspace_id): AxumPath<String>,
    State(state): State<AppState>,
    Json(input): Json<CanvasCommentOpRequest>,
) -> Result<Json<Value>, ApiError> {
    let workspace = state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    let mut store = read_comment_store(&state.data_dir, &workspace_id).await?;
    store.seq = store
        .seq
        .max(current_workspace_version_seq(&state, workspace.cur_version_id.as_deref()).await?);
    apply_comment_op(&mut store, input.op)?;
    write_comment_store(&state.data_dir, &workspace_id, &store).await?;
    Ok(Json(workspace_canvas_value(&state, &workspace_id).await?))
}

pub(crate) async fn update_canvas_presence(
    AxumPath(workspace_id): AxumPath<String>,
    State(state): State<AppState>,
    Json(input): Json<CanvasPresencePayload>,
) -> Result<Json<Value>, ApiError> {
    state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    validate_actor(&input.actor)?;
    validate_presence(&input)?;
    let event = RunEventEnvelope {
        workspace_id,
        run_id: "canvas_presence".to_owned(),
        seq: 0,
        server_time: now_marker(),
        ev: "canvas.presence".to_owned(),
        data: serde_json::to_value(&input).map_err(|err| {
            ApiError::server_error(format!("encode canvas presence event: {err}"))
        })?,
    };
    drop(state.events.publish(event.clone()));
    Ok(Json(json!({
        "ok": true,
        "event": event.ev,
        "actorId": input.actor.actor_id,
    })))
}

pub(crate) async fn load_canvas_comments(
    data_dir: &Path,
    workspace_id: &str,
) -> Result<Vec<CanvasComment>, ApiError> {
    Ok(read_comment_store(data_dir, workspace_id).await?.comments)
}

pub(crate) async fn canvas_comment_seq(
    data_dir: &Path,
    workspace_id: &str,
) -> Result<i64, ApiError> {
    Ok(read_comment_store(data_dir, workspace_id).await?.seq)
}

async fn read_comment_store(
    data_dir: &Path,
    workspace_id: &str,
) -> Result<CanvasCommentStore, ApiError> {
    let relative = comments_relative_path(workspace_id)?;
    let full_path = data_dir.join(&relative);
    let bytes = match tokio::fs::read(&full_path).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CanvasCommentStore::default());
        }
        Err(err) => {
            return Err(ApiError::io(
                format!("read canvas comments `{}`", relative.display()),
                err,
            ));
        }
    };
    let store: CanvasCommentStore = serde_json::from_slice(&bytes).map_err(|err| {
        ApiError::server_error(format!(
            "canvas comments `{}` contains invalid JSON: {err}",
            relative.display()
        ))
    })?;
    if store.seq < 0 {
        return Err(ApiError::server_error(format!(
            "canvas comments `{}` has a negative seq",
            relative.display()
        )));
    }
    if store.comments.len() as i64 > store.seq {
        return Err(ApiError::server_error(format!(
            "canvas comments `{}` snapshot is ahead of seq",
            relative.display()
        )));
    }
    Ok(store)
}

async fn write_comment_store(
    data_dir: &Path,
    workspace_id: &str,
    store: &CanvasCommentStore,
) -> Result<(), ApiError> {
    let relative = comments_relative_path(workspace_id)?;
    write_json_file(data_dir, &relative, store, "write canvas comments").await?;
    Ok(())
}

async fn current_workspace_version_seq(
    state: &AppState,
    version_id: Option<&str>,
) -> Result<i64, ApiError> {
    let Some(version_id) = version_id else {
        return Ok(0);
    };
    let version = state
        .store
        .version(version_id)
        .await
        .map_err(ApiError::store)?;
    Ok(version.idx)
}

fn comments_relative_path(workspace_id: &str) -> Result<PathBuf, ApiError> {
    if workspace_id.is_empty()
        || !workspace_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(ApiError::bad_request("workspace id is not path safe"));
    }
    Ok(PathBuf::from("canvas")
        .join(workspace_id)
        .join("comments.json"))
}

fn apply_comment_op(store: &mut CanvasCommentStore, op: CanvasCommentOp) -> Result<(), ApiError> {
    match op {
        CanvasCommentOp::CommentAdd {
            id,
            target,
            body,
            actor,
        } => {
            let body = normalize_body(body)?;
            let actor = actor.unwrap_or_else(local_actor);
            validate_actor(&actor)?;
            validate_target(&target)?;
            let id = id
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| format!("comment_{}", Uuid::now_v7()));
            if store.comments.iter().any(|comment| comment.id == id) {
                return Err(ApiError::conflict(format!("comment already exists: {id}")));
            }
            let now = now_marker();
            store.comments.push(CanvasComment {
                id,
                target,
                body,
                author: actor,
                status: CanvasCommentStatus::Open,
                created_at: now.clone(),
                updated_at: now,
            });
        }
        CanvasCommentOp::CommentPatch { id, body, status } => {
            let comment = store
                .comments
                .iter_mut()
                .find(|comment| comment.id == id)
                .ok_or_else(|| ApiError::not_found(format!("comment was not found: {id}")))?;
            if let Some(body) = body {
                comment.body = normalize_body(body)?;
            }
            if let Some(status) = status {
                comment.status = status;
            }
            comment.updated_at = now_marker();
        }
        CanvasCommentOp::CommentDelete { id } => {
            let before = store.comments.len();
            store.comments.retain(|comment| comment.id != id);
            if store.comments.len() == before {
                return Err(ApiError::not_found(format!("comment was not found: {id}")));
            }
        }
    }
    store.seq += 1;
    Ok(())
}

fn normalize_body(body: String) -> Result<String, ApiError> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err(ApiError::bad_request("comment body must not be empty"));
    }
    Ok(trimmed.to_owned())
}

fn validate_actor(actor: &CanvasActor) -> Result<(), ApiError> {
    if actor.actor_id.trim().is_empty() {
        return Err(ApiError::bad_request("actorId must not be empty"));
    }
    if actor.display_name.trim().is_empty() {
        return Err(ApiError::bad_request("displayName must not be empty"));
    }
    Ok(())
}

fn validate_target(target: &CanvasCommentTarget) -> Result<(), ApiError> {
    match target {
        CanvasCommentTarget::Node { node_id } if node_id.trim().is_empty() => Err(
            ApiError::bad_request("comment target nodeId must not be empty"),
        ),
        CanvasCommentTarget::Edge { edge_id } if edge_id.trim().is_empty() => Err(
            ApiError::bad_request("comment target edgeId must not be empty"),
        ),
        CanvasCommentTarget::Position { x, y } if !x.is_finite() || !y.is_finite() => Err(
            ApiError::bad_request("comment target position must contain finite numbers"),
        ),
        _ => Ok(()),
    }
}

fn validate_presence(input: &CanvasPresencePayload) -> Result<(), ApiError> {
    if let Some(cursor) = &input.cursor
        && (!cursor.x.is_finite() || !cursor.y.is_finite())
    {
        return Err(ApiError::bad_request(
            "presence cursor must contain finite numbers",
        ));
    }
    if let Some(viewport) = &input.viewport
        && (!viewport.x.is_finite() || !viewport.y.is_finite() || !viewport.zoom.is_finite())
    {
        return Err(ApiError::bad_request(
            "presence viewport must contain finite numbers",
        ));
    }
    Ok(())
}

fn local_actor() -> CanvasActor {
    CanvasActor {
        actor_id: "local".to_owned(),
        display_name: "Local user".to_owned(),
    }
}

fn now_marker() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format!("unix:{seconds}")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::Json;
    use axum::extract::{Path as AxumPath, State};
    use helixflow_run::EventBus;
    use helixflow_store::Store;

    use super::*;
    use crate::test_support::FailingWorkbenchAgent;

    #[tokio::test]
    async fn comment_ops_persist_and_reload_from_canvas_snapshot() {
        let (_dir, state, workspace_id) = test_state().await;

        let added = apply_canvas_comment_op(
            AxumPath(workspace_id.clone()),
            State(state.clone()),
            Json(CanvasCommentOpRequest {
                base_seq: None,
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
    async fn presence_broadcasts_without_creating_durable_comments() {
        let (_dir, state, workspace_id) = test_state().await;
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
        assert_eq!(
            load_canvas_comments(&state.data_dir, &workspace_id)
                .await
                .expect("comments")
                .len(),
            0
        );
    }

    async fn test_state() -> (tempfile::TempDir, AppState, String) {
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
}
