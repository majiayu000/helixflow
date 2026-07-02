use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use axum::{
    Json,
    extract::{Path as AxumPath, State},
};
use helixflow_graph::{GraphEdge, GraphError, GraphNode, GraphService, ProposalOp, WorkflowGraph};
use helixflow_registry::NodeRegistry;
use helixflow_store::{NewVersion, StoreError, VersionSource};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::{graph_hash, read_graph_file};
use crate::workspace_state::workspace_state_value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OpsRequest {
    base_version_id: String,
    label: Option<String>,
    ops: Vec<ManualOpRequest>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum ManualOpRequest {
    AddNode {
        id: String,
        node_type: String,
        title: Option<String>,
        params: Value,
        pos: [f32; 2],
    },
    RemoveNode {
        id: String,
    },
    SetParam {
        id: String,
        key: String,
        value: Value,
    },
    AddEdge {
        from: [String; 2],
        to: [String; 2],
        edge_type: String,
    },
    RemoveEdge {
        from: [String; 2],
        to: [String; 2],
        edge_type: String,
    },
    MoveNode {
        id: String,
        pos: [f32; 2],
    },
}

pub(crate) async fn apply_workspace_ops(
    AxumPath(workspace_id): AxumPath<String>,
    State(state): State<AppState>,
    body: String,
) -> Result<Json<Value>, ApiError> {
    let input: OpsRequest = serde_json::from_str(&body)
        .map_err(|err| ApiError::bad_request(format!("invalid ops request: {err}")))?;
    if input.base_version_id.trim().is_empty() {
        return Err(ApiError::bad_request("baseVersionId is required"));
    }
    if input.ops.is_empty() {
        return Err(ApiError::bad_request("ops must contain at least one op"));
    }

    let workspace = state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    let current_version_id = workspace
        .cur_version_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("workspace has no current version"))?;
    if input.base_version_id != current_version_id {
        return Err(ApiError::conflict(format!(
            "ops base `{}` is superseded by `{current_version_id}`",
            input.base_version_id
        )));
    }
    if state
        .store
        .latest_pending_proposal(&workspace_id)
        .await
        .map_err(ApiError::store)?
        .is_some()
    {
        return Err(ApiError::conflict(
            "workspace has a pending proposal; apply or dismiss it before editing",
        ));
    }

    let current = state
        .store
        .version(current_version_id)
        .await
        .map_err(ApiError::store)?;
    let current_graph = read_graph_file(&state.data_dir, &current.graph_path).await?;
    let registry = NodeRegistry::builtin();
    let graph_service = GraphService::new(registry.clone());
    let (ops, edited_graph) = prepare_ops(input.ops, &current_graph, &registry, &graph_service)?;
    graph_service
        .validate_graph(&edited_graph)
        .map_err(|err| graph_ops_error(err, None))?;

    let graph_path = ops_graph_path(&workspace_id);
    let graph_hash = write_graph_atomically(&state.data_dir, &graph_path, &edited_graph).await?;
    let graph_path_string = graph_path.to_string_lossy().into_owned();
    let label = input
        .label
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| manual_version_label(&ops));
    let version_result = state
        .store
        .create_version_after_without_pending_proposal(
            NewVersion {
                workspace_id: &workspace_id,
                label: &label,
                source: VersionSource::Manual,
                graph_path: &graph_path_string,
                graph_hash: &graph_hash,
                parent_id: Some(current_version_id),
            },
            current_version_id,
        )
        .await;

    match version_result {
        Ok(_) => Ok(Json(workspace_state_value(&state, &workspace_id).await?)),
        Err(StoreError::VersionConflict { .. } | StoreError::PendingProposalConflict { .. }) => {
            cleanup_graph_files(&state.data_dir, &graph_path).await;
            Err(ApiError::conflict(
                "workspace changed while applying manual ops; refresh and retry",
            ))
        }
        Err(err) => {
            cleanup_graph_files(&state.data_dir, &graph_path).await;
            Err(ApiError::store(err))
        }
    }
}

fn prepare_ops(
    requests: Vec<ManualOpRequest>,
    base_graph: &WorkflowGraph,
    registry: &NodeRegistry,
    graph_service: &GraphService,
) -> Result<(Vec<ProposalOp>, WorkflowGraph), ApiError> {
    let mut graph = base_graph.clone();
    let mut ops = Vec::new();
    let mut set_params = BTreeSet::new();
    for (index, request) in requests.into_iter().enumerate() {
        if let ManualOpRequest::SetParam { id, key, .. } = &request
            && !set_params.insert((id.clone(), key.clone()))
        {
            return Err(op_error(
                index,
                format!("duplicate set_param for `{id}.{key}`"),
            ));
        }
        let op = manual_op_to_proposal_op(request, &graph, registry)
            .map_err(|err| op_error(index, err.message))?;
        graph = graph_service
            .apply_ops(&graph, std::slice::from_ref(&op))
            .map_err(|err| graph_ops_error(err, Some(index)))?;
        ops.push(op);
    }
    Ok((ops, graph))
}

fn manual_op_to_proposal_op(
    op: ManualOpRequest,
    base_graph: &WorkflowGraph,
    registry: &NodeRegistry,
) -> Result<ProposalOp, ApiError> {
    match op {
        ManualOpRequest::AddNode {
            id,
            node_type,
            title,
            params,
            pos,
        } => {
            ensure_non_empty("node id", &id)?;
            ensure_finite_pos(pos)?;
            let definition = registry
                .definition(&node_type)
                .map_err(|err| ApiError::bad_request(err.to_string()))?;
            Ok(ProposalOp::AddNode {
                id,
                node: GraphNode {
                    node_type,
                    title: title
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| definition.title.clone()),
                    params,
                    pos,
                },
            })
        }
        ManualOpRequest::RemoveNode { id } => {
            ensure_non_empty("node id", &id)?;
            Ok(ProposalOp::RemoveNode { id })
        }
        ManualOpRequest::SetParam { id, key, value } => {
            ensure_non_empty("node id", &id)?;
            ensure_non_empty("param key", &key)?;
            let prev = base_graph
                .nodes
                .get(&id)
                .and_then(|node| node.params.as_object())
                .and_then(|params| params.get(&key))
                .cloned();
            Ok(ProposalOp::SetParam {
                id,
                key,
                prev,
                value,
            })
        }
        ManualOpRequest::AddEdge {
            from,
            to,
            edge_type,
        } => {
            ensure_edge_parts(&from, &to, &edge_type)?;
            Ok(ProposalOp::AddEdge {
                edge: GraphEdge {
                    from,
                    to,
                    edge_type,
                },
            })
        }
        ManualOpRequest::RemoveEdge {
            from,
            to,
            edge_type,
        } => {
            ensure_edge_parts(&from, &to, &edge_type)?;
            Ok(ProposalOp::RemoveEdge {
                edge: GraphEdge {
                    from,
                    to,
                    edge_type,
                },
            })
        }
        ManualOpRequest::MoveNode { id, pos } => {
            ensure_non_empty("node id", &id)?;
            ensure_finite_pos(pos)?;
            Ok(ProposalOp::MoveNode { id, pos })
        }
    }
}

fn op_error(index: usize, message: impl Into<String>) -> ApiError {
    ApiError::bad_request_with_details(message, json!({ "opIndex": index }))
}

fn graph_ops_error(err: GraphError, index: Option<usize>) -> ApiError {
    match err {
        GraphError::ProposalSuperseded { .. } => ApiError::conflict(err.to_string()),
        _ => ApiError::bad_request_with_details(err.to_string(), json!({ "opIndex": index })),
    }
}

fn ensure_non_empty(label: &str, value: &str) -> Result<(), ApiError> {
    if value.trim().is_empty() {
        return Err(ApiError::bad_request(format!("{label} is required")));
    }
    Ok(())
}

fn ensure_finite_pos(pos: [f32; 2]) -> Result<(), ApiError> {
    if pos.iter().any(|value| !value.is_finite()) {
        return Err(ApiError::bad_request(
            "node position must contain finite numbers",
        ));
    }
    Ok(())
}

fn ensure_edge_parts(
    from: &[String; 2],
    to: &[String; 2],
    edge_type: &str,
) -> Result<(), ApiError> {
    ensure_non_empty("from node", &from[0])?;
    ensure_non_empty("from port", &from[1])?;
    ensure_non_empty("to node", &to[0])?;
    ensure_non_empty("to port", &to[1])?;
    ensure_non_empty("edge type", edge_type)?;
    Ok(())
}

fn manual_version_label(ops: &[ProposalOp]) -> String {
    match ops {
        [op] => manual_title(op),
        _ => format!("Manual edit ({} ops)", ops.len()),
    }
}

fn manual_title(op: &ProposalOp) -> String {
    match op {
        ProposalOp::AddNode { id, .. } => format!("Add node {id}"),
        ProposalOp::RemoveNode { id } => format!("Remove node {id}"),
        ProposalOp::SetParam { id, key, .. } => format!("Edit {id}.{key}"),
        ProposalOp::AddEdge { edge } => format!(
            "Connect {}.{} to {}.{}",
            edge.from[0], edge.from[1], edge.to[0], edge.to[1]
        ),
        ProposalOp::RemoveEdge { edge } => format!(
            "Disconnect {}.{} from {}.{}",
            edge.from[0], edge.from[1], edge.to[0], edge.to[1]
        ),
        ProposalOp::MoveNode { id, .. } => format!("Move node {id}"),
    }
}

fn ops_graph_path(workspace_id: &str) -> PathBuf {
    PathBuf::from("workspaces")
        .join(workspace_id)
        .join("graphs")
        .join(format!("ops-{}.json", Uuid::now_v7().simple()))
}

async fn write_graph_atomically(
    data_dir: &Path,
    graph_path: &Path,
    graph: &WorkflowGraph,
) -> Result<String, ApiError> {
    let bytes = serde_json::to_vec_pretty(graph)
        .map_err(|err| ApiError::server_error(format!("write ops graph: encode JSON: {err}")))?;
    let full_path = data_dir.join(graph_path);
    let tmp_path = tmp_graph_path(&full_path);
    if let Some(parent) = full_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| ApiError::io("write ops graph: create parent directory", err))?;
    }
    tokio::fs::write(&tmp_path, &bytes)
        .await
        .map_err(|err| ApiError::io("write ops graph: write temp file", err))?;
    tokio::fs::rename(&tmp_path, &full_path)
        .await
        .map_err(|err| ApiError::io("write ops graph: rename temp file", err))?;
    Ok(graph_hash(&bytes))
}

async fn cleanup_graph_files(data_dir: &Path, graph_path: &Path) {
    let full_path = data_dir.join(graph_path);
    let tmp_path = tmp_graph_path(&full_path);
    drop(tokio::fs::remove_file(&full_path).await);
    drop(tokio::fs::remove_file(&tmp_path).await);
}

fn tmp_graph_path(full_path: &Path) -> PathBuf {
    let mut tmp = full_path.as_os_str().to_owned();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}
