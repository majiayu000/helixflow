use std::collections::{BTreeMap, BTreeSet};

use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_graph::WorkflowGraph;
use helixflow_store::{CanvasSnapshotCommitResult, CanvasSnapshotRecord, CommitCanvasSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::canvas_collaboration::CanvasViewport;
use crate::version_file_consistency::read_version_graph;
use crate::workspace_canvas::workspace_canvas_value;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CanvasSnapshot {
    #[serde(default)]
    pub(crate) nodes: BTreeMap<String, CanvasNodeLayout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) viewport: Option<CanvasViewport>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CanvasNodeLayout {
    pub(crate) position: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) size: Option<[f32; 2]>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SaveCanvasSnapshotRequest {
    pub(crate) version_id: String,
    pub(crate) base_revision: i64,
    #[serde(default)]
    pub(crate) positions: Vec<NodePositionUpdate>,
    #[serde(default)]
    pub(crate) sizes: Vec<NodeSizeUpdate>,
    #[serde(default)]
    pub(crate) viewport: Option<CanvasViewport>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NodePositionUpdate {
    pub(crate) id: String,
    pub(crate) x: f32,
    pub(crate) y: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NodeSizeUpdate {
    pub(crate) id: String,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

pub(crate) async fn save_canvas_snapshot(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
    Json(request): Json<SaveCanvasSnapshotRequest>,
) -> Result<Json<Value>, ApiError> {
    validate_request_shape(&request)?;
    let workspace = state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    let current_version_id = workspace
        .cur_version_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("workspace has no current version"))?;
    if request.version_id != current_version_id {
        return Err(ApiError::conflict(format!(
            "canvas graph version `{}` is stale; current version is `{current_version_id}`",
            request.version_id
        )));
    }
    let version = state
        .store
        .version(current_version_id)
        .await
        .map_err(ApiError::store)?;
    let graph = read_version_graph(&state.data_dir, &version)
        .await
        .map_err(|error| ApiError::server_error(error.to_string()))?;
    let current_record = state
        .store
        .canvas_snapshot(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    let current_revision = current_record.as_ref().map_or(0, |record| record.revision);
    if request.base_revision != current_revision {
        return Err(stale_revision(request.base_revision, current_revision));
    }

    let mut snapshot = snapshot_for_graph(current_record.as_ref(), &graph)?;
    apply_positions(&mut snapshot, &graph, request.positions)?;
    apply_sizes(&mut snapshot, &graph, request.sizes)?;
    if let Some(viewport) = request.viewport {
        validate_viewport(viewport)?;
        snapshot.viewport = Some(viewport);
    }
    let nodes_json = serde_json::to_string(&snapshot.nodes)
        .map_err(|error| ApiError::server_error(format!("encode canvas nodes: {error}")))?;
    let viewport_json = snapshot
        .viewport
        .map(|viewport| {
            serde_json::to_string(&viewport)
                .map_err(|error| ApiError::server_error(format!("encode canvas viewport: {error}")))
        })
        .transpose()?;

    match state
        .store
        .commit_canvas_snapshot(CommitCanvasSnapshot {
            workspace_id: &workspace_id,
            expected_revision: request.base_revision,
            nodes_json: &nodes_json,
            viewport_json: viewport_json.as_deref(),
        })
        .await
        .map_err(ApiError::store)?
    {
        CanvasSnapshotCommitResult::Applied(_) => {
            Ok(Json(workspace_canvas_value(&state, &workspace_id).await?))
        }
        CanvasSnapshotCommitResult::Stale { current_revision } => {
            Err(stale_revision(request.base_revision, current_revision))
        }
    }
}

pub(crate) fn snapshot_from_record(
    record: Option<&CanvasSnapshotRecord>,
) -> Result<(i64, CanvasSnapshot), ApiError> {
    let Some(record) = record else {
        return Ok((0, CanvasSnapshot::default()));
    };
    let nodes = serde_json::from_str(&record.nodes_json)
        .map_err(|error| ApiError::server_error(format!("decode canvas nodes: {error}")))?;
    let viewport = record
        .viewport_json
        .as_deref()
        .map(|value| {
            serde_json::from_str(value)
                .map_err(|error| ApiError::server_error(format!("decode canvas viewport: {error}")))
        })
        .transpose()?;
    Ok((record.revision, CanvasSnapshot { nodes, viewport }))
}

fn snapshot_for_graph(
    record: Option<&CanvasSnapshotRecord>,
    graph: &WorkflowGraph,
) -> Result<CanvasSnapshot, ApiError> {
    let (_, persisted) = snapshot_from_record(record)?;
    Ok(CanvasSnapshot {
        nodes: graph
            .nodes
            .iter()
            .map(|(id, node)| {
                let layout = persisted
                    .nodes
                    .get(id)
                    .cloned()
                    .unwrap_or(CanvasNodeLayout {
                        position: node.pos,
                        size: node.size,
                    });
                (id.clone(), layout)
            })
            .collect(),
        viewport: persisted.viewport,
    })
}

fn validate_request_shape(request: &SaveCanvasSnapshotRequest) -> Result<(), ApiError> {
    if request.base_revision < 0 {
        return Err(ApiError::bad_request("baseRevision must not be negative"));
    }
    if request.positions.is_empty() && request.sizes.is_empty() && request.viewport.is_none() {
        return Err(ApiError::bad_request(
            "canvas snapshot update must include positions, sizes, or viewport",
        ));
    }
    Ok(())
}

fn apply_positions(
    snapshot: &mut CanvasSnapshot,
    graph: &WorkflowGraph,
    positions: Vec<NodePositionUpdate>,
) -> Result<(), ApiError> {
    let mut seen = BTreeSet::new();
    for position in positions {
        validate_node_id(&position.id, &mut seen, "position")?;
        if !position.x.is_finite() || !position.y.is_finite() {
            return Err(ApiError::bad_request(format!(
                "canvas position for node `{}` must be finite",
                position.id
            )));
        }
        let layout = snapshot.nodes.get_mut(&position.id).ok_or_else(|| {
            ApiError::bad_request(format!("unknown canvas node `{}`", position.id))
        })?;
        debug_assert!(graph.nodes.contains_key(&position.id));
        layout.position = [position.x, position.y];
    }
    Ok(())
}

fn apply_sizes(
    snapshot: &mut CanvasSnapshot,
    graph: &WorkflowGraph,
    sizes: Vec<NodeSizeUpdate>,
) -> Result<(), ApiError> {
    let mut seen = BTreeSet::new();
    for size in sizes {
        validate_node_id(&size.id, &mut seen, "size")?;
        if !size.width.is_finite()
            || !size.height.is_finite()
            || size.width <= 0.0
            || size.height <= 0.0
        {
            return Err(ApiError::bad_request(format!(
                "canvas size for node `{}` must be finite and positive",
                size.id
            )));
        }
        let layout = snapshot
            .nodes
            .get_mut(&size.id)
            .ok_or_else(|| ApiError::bad_request(format!("unknown canvas node `{}`", size.id)))?;
        debug_assert!(graph.nodes.contains_key(&size.id));
        layout.size = Some([size.width, size.height]);
    }
    Ok(())
}

fn validate_node_id(id: &str, seen: &mut BTreeSet<String>, field: &str) -> Result<(), ApiError> {
    if id.trim().is_empty() {
        return Err(ApiError::bad_request(format!(
            "canvas {field} node id cannot be empty"
        )));
    }
    if !seen.insert(id.to_owned()) {
        return Err(ApiError::bad_request(format!(
            "duplicate canvas {field} for node `{id}`"
        )));
    }
    Ok(())
}

fn validate_viewport(viewport: CanvasViewport) -> Result<(), ApiError> {
    if !viewport.x.is_finite()
        || !viewport.y.is_finite()
        || !viewport.zoom.is_finite()
        || viewport.zoom <= 0.0
    {
        return Err(ApiError::bad_request(
            "canvas viewport values must be finite and zoom must be positive",
        ));
    }
    Ok(())
}

fn stale_revision(expected: i64, actual: i64) -> ApiError {
    ApiError::conflict_with_details(
        format!("canvas revision `{expected}` is stale; current revision is `{actual}`"),
        serde_json::json!({ "currentRevision": actual }),
    )
}
