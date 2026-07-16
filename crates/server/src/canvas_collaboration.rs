use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path as AxumPath, State},
};
use helixflow_run::RunEventEnvelope;
use helixflow_store::{
    CanvasCommentCommitResult, CanvasCommentOperationRecord, CanvasCommentStateRecord,
    CommitCanvasCommentOperation, NewCanvasCommentState, Store,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::workspace_canvas::workspace_canvas_value;

// Covers the 40-way compatibility acceptance scale with room for extra collaborators.
const MAX_COMPATIBILITY_CAS_RETRIES: usize = 64;

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
    base_seq: Option<i64>,
    operation_id: Option<String>,
    op: CanvasCommentOp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    if input.base_seq.is_some_and(|seq| seq < 0) {
        return Err(ApiError::bad_request("baseSeq must not be negative"));
    }
    let operation_id = normalize_operation_id(input.operation_id)?;
    let op = prepare_comment_op(input.op, &operation_id)?;
    let operation_fingerprint = comment_operation_fingerprint(&op)?;
    let mut current = ensure_comment_store(&state.store, &state.data_dir, &workspace_id).await?;

    if let Some(existing) = state
        .store
        .canvas_comment_operation(&workspace_id, &operation_id)
        .await
        .map_err(ApiError::store)?
    {
        return replay_or_conflict(&state, &workspace_id, &operation_fingerprint, existing).await;
    }

    let mut compatibility_cas_retries = 0;
    loop {
        if let Some(base_seq) = input.base_seq
            && base_seq != current.seq
        {
            return Err(comment_conflict(
                "canvas comment base sequence is stale",
                current.seq,
            ));
        }
        let expected_seq = input.base_seq.unwrap_or(current.seq);
        let mut next = current;
        apply_comment_op(&mut next, op.clone())?;
        let comments_json = serde_json::to_string(&next.comments)
            .map_err(|err| ApiError::server_error(format!("encode canvas comments: {err}")))?;
        match state
            .store
            .commit_canvas_comment_operation(CommitCanvasCommentOperation {
                workspace_id: &workspace_id,
                operation_id: &operation_id,
                operation_fingerprint: &operation_fingerprint,
                expected_seq,
                comments_json: &comments_json,
            })
            .await
            .map_err(ApiError::store)?
        {
            CanvasCommentCommitResult::Applied(_) | CanvasCommentCommitResult::Replayed(_) => {
                return Ok(Json(workspace_canvas_value(&state, &workspace_id).await?));
            }
            CanvasCommentCommitResult::Stale { current_seq } => {
                if input.base_seq.is_none() {
                    prepare_compatibility_cas_retry(&mut compatibility_cas_retries, current_seq)?;
                    tokio::task::yield_now().await;
                    current = read_comment_store(&state.store, &workspace_id).await?;
                    continue;
                }
                return Err(comment_conflict(
                    "canvas comment base sequence is stale",
                    current_seq,
                ));
            }
            CanvasCommentCommitResult::OperationIdConflict { current_seq } => {
                return Err(comment_conflict(
                    "canvas comment operationId was reused with different content",
                    current_seq,
                ));
            }
        }
    }
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

pub(crate) async fn canvas_comment_snapshot(
    store: &Store,
    data_dir: &Path,
    workspace_id: &str,
) -> Result<(i64, Vec<CanvasComment>), ApiError> {
    let snapshot = ensure_comment_store(store, data_dir, workspace_id).await?;
    Ok((snapshot.seq, snapshot.comments))
}

async fn ensure_comment_store(
    store: &Store,
    data_dir: &Path,
    workspace_id: &str,
) -> Result<CanvasCommentStore, ApiError> {
    if let Some(record) = store
        .canvas_comment_state(workspace_id)
        .await
        .map_err(ApiError::store)?
    {
        return comment_store_from_record(record);
    }

    let relative = comments_relative_path(workspace_id)?;
    let full_path = data_dir.join(&relative);
    let (legacy, migration_source) = match tokio::fs::read(&full_path).await {
        Ok(bytes) => {
            let legacy: CanvasCommentStore = serde_json::from_slice(&bytes).map_err(|err| {
                ApiError::server_error(format!(
                    "canvas comments `{}` contains invalid JSON: {err}",
                    relative.display()
                ))
            })?;
            validate_comment_store(&legacy, &relative)?;
            (legacy, "legacy_json")
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            (CanvasCommentStore::default(), "empty")
        }
        Err(err) => {
            return Err(ApiError::io(
                format!("read canvas comments `{}`", relative.display()),
                err,
            ));
        }
    };
    let comments_json = serde_json::to_string(&legacy.comments)
        .map_err(|err| ApiError::server_error(format!("encode legacy canvas comments: {err}")))?;
    let record = store
        .initialize_canvas_comment_state(NewCanvasCommentState {
            workspace_id,
            seq: legacy.seq,
            comments_json: &comments_json,
            migration_source,
        })
        .await
        .map_err(ApiError::store)?;
    comment_store_from_record(record)
}

async fn read_comment_store(
    store: &Store,
    workspace_id: &str,
) -> Result<CanvasCommentStore, ApiError> {
    let record = store
        .canvas_comment_state(workspace_id)
        .await
        .map_err(ApiError::store)?
        .ok_or_else(|| ApiError::server_error("canvas comments were not initialized"))?;
    comment_store_from_record(record)
}

fn comment_store_from_record(
    record: CanvasCommentStateRecord,
) -> Result<CanvasCommentStore, ApiError> {
    let comments: Vec<CanvasComment> =
        serde_json::from_str(&record.comments_json).map_err(|err| {
            ApiError::server_error(format!(
                "canvas comments in SQLite for workspace `{}` contain invalid JSON: {err}",
                record.workspace_id
            ))
        })?;
    let store = CanvasCommentStore {
        schema_version: 1,
        seq: record.seq,
        comments,
    };
    validate_comment_store(&store, Path::new("sqlite:canvas_comment_states"))?;
    Ok(store)
}

fn validate_comment_store(store: &CanvasCommentStore, source: &Path) -> Result<(), ApiError> {
    if store.schema_version != 1 {
        return Err(ApiError::server_error(format!(
            "canvas comments `{}` has unsupported schema version {}",
            source.display(),
            store.schema_version
        )));
    }
    if store.seq < 0 {
        return Err(ApiError::server_error(format!(
            "canvas comments `{}` has a negative seq",
            source.display()
        )));
    }
    if store.comments.len() as i64 > store.seq {
        return Err(ApiError::server_error(format!(
            "canvas comments `{}` snapshot is ahead of seq",
            source.display()
        )));
    }
    Ok(())
}

async fn replay_or_conflict(
    state: &AppState,
    workspace_id: &str,
    operation_fingerprint: &str,
    existing: CanvasCommentOperationRecord,
) -> Result<Json<Value>, ApiError> {
    if existing.operation_fingerprint == operation_fingerprint {
        return Ok(Json(workspace_canvas_value(state, workspace_id).await?));
    }
    let current_seq = read_comment_store(&state.store, workspace_id).await?.seq;
    Err(comment_conflict(
        "canvas comment operationId was reused with different content",
        current_seq,
    ))
}

fn comment_conflict(message: &str, current_seq: i64) -> ApiError {
    ApiError::conflict_with_details(message, json!({ "currentSeq": current_seq }))
}

fn prepare_compatibility_cas_retry(
    completed_retries: &mut usize,
    current_seq: i64,
) -> Result<(), ApiError> {
    if *completed_retries >= MAX_COMPATIBILITY_CAS_RETRIES {
        return Err(comment_conflict(
            "canvas comment changed repeatedly; refresh and retry",
            current_seq,
        ));
    }
    *completed_retries += 1;
    Ok(())
}

fn normalize_operation_id(operation_id: Option<String>) -> Result<String, ApiError> {
    let Some(operation_id) = operation_id else {
        return Ok(format!("operation_{}", Uuid::now_v7()));
    };
    let operation_id = operation_id.trim();
    if operation_id.is_empty() {
        return Err(ApiError::bad_request("operationId must not be empty"));
    }
    if operation_id.len() > 256 {
        return Err(ApiError::bad_request(
            "operationId must not exceed 256 bytes",
        ));
    }
    Ok(operation_id.to_owned())
}

fn prepare_comment_op(
    op: CanvasCommentOp,
    operation_id: &str,
) -> Result<CanvasCommentOp, ApiError> {
    match op {
        CanvasCommentOp::CommentAdd {
            id,
            target,
            body,
            actor,
        } => {
            validate_target(&target)?;
            let actor = actor.unwrap_or_else(local_actor);
            validate_actor(&actor)?;
            Ok(CanvasCommentOp::CommentAdd {
                id: Some(
                    id.filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| stable_comment_id(operation_id)),
                ),
                target,
                body: normalize_body(body)?,
                actor: Some(actor),
            })
        }
        CanvasCommentOp::CommentPatch { id, body, status } => Ok(CanvasCommentOp::CommentPatch {
            id,
            body: body.map(normalize_body).transpose()?,
            status,
        }),
        CanvasCommentOp::CommentDelete { id } => Ok(CanvasCommentOp::CommentDelete { id }),
    }
}

fn stable_comment_id(operation_id: &str) -> String {
    let digest = Sha256::digest(operation_id.as_bytes());
    format!("comment_{digest:x}")
}

fn comment_operation_fingerprint(op: &CanvasCommentOp) -> Result<String, ApiError> {
    let encoded = serde_json::to_vec(op)
        .map_err(|err| ApiError::server_error(format!("encode canvas comment operation: {err}")))?;
    let digest = Sha256::digest(encoded);
    Ok(format!("sha256:{digest:x}"))
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
#[path = "canvas_collaboration_tests.rs"]
mod tests;
