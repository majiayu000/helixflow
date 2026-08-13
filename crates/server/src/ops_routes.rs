use std::collections::BTreeSet;
use std::io;

use axum::{
    Json,
    extract::{Path as AxumPath, State},
};
use helixflow_graph::{GraphEdge, GraphError, GraphNode, GraphService, ProposalOp, WorkflowGraph};
use helixflow_registry::NodeRegistry;
use helixflow_store::{NewVersion, StoreError, VersionSource};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::version_file_consistency::{
    CandidateKind, VersionFileCandidate, VersionFileConsistencyError, read_version_graph,
};
use crate::version_semantics::derive_semantics_json;
use crate::workspace_state::workspace_state_value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OpsRequest {
    base_version_id: String,
    idempotency_key: Option<String>,
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
        prev: Option<Value>,
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
    ResizeNode {
        id: String,
        size: [f32; 2],
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
    if let Some(key) = input.idempotency_key.as_deref() {
        ensure_idempotency_key(key)?;
    }
    if input.ops.is_empty() {
        return Err(ApiError::bad_request("ops must contain at least one op"));
    }
    let workspace = state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    workspace
        .cur_version_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("workspace has no current version"))?;
    let base = state
        .store
        .version(&input.base_version_id)
        .await
        .map_err(ApiError::store)?;
    if base.workspace_id != workspace_id {
        return Err(ApiError::conflict(
            "ops base version belongs to another workspace",
        ));
    }
    let current_graph = read_version_graph(&state.data_dir, &base)
        .await
        .map_err(candidate_error)?;
    let registry = NodeRegistry::builtin();
    let graph_service = GraphService::new(registry.clone());
    let (ops, edited_graph) = prepare_ops(input.ops, &current_graph, &registry, &graph_service)?;
    graph_service
        .validate_graph(&edited_graph)
        .map_err(|err| graph_ops_error(err, None))?;

    let label = input
        .label
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| manual_version_label(&ops));
    let opaque_digest = input
        .idempotency_key
        .as_deref()
        .map(|key| ops_idempotency_digest(&workspace_id, &input.base_version_id, key));
    let mut candidate = match opaque_digest.as_deref() {
        Some(digest) => {
            VersionFileCandidate::from_keyed_ops_graph(&workspace_id, &edited_graph, digest)
        }
        None => VersionFileCandidate::from_graph(&workspace_id, CandidateKind::Ops, &edited_graph),
    }
    .map_err(candidate_error)?;
    let graph_path = candidate
        .relative_path_text()
        .map_err(candidate_error)?
        .to_owned();
    let graph_hash = candidate.graph_hash().to_owned();
    let semantics_json = derive_semantics_json(&base, &current_graph, &edited_graph)?;
    if let Err(error) = candidate.publish(&state.data_dir) {
        if opaque_digest.is_some() && is_publish_collision(&error) {
            if accepted_idempotent_version(
                &state,
                &workspace_id,
                &input.base_version_id,
                &graph_path,
                &graph_hash,
            )
            .await?
            {
                return Ok(Json(workspace_state_value(&state, &workspace_id).await?));
            }
            return Err(ApiError::conflict(
                "idempotent ops candidate conflicts with existing state",
            ));
        }
        return Err(candidate_error(error));
    }
    let version_result = state
        .store
        .create_version_after_without_pending_proposal(
            NewVersion {
                workspace_id: &workspace_id,
                label: &label,
                source: VersionSource::Manual,
                graph_path: &graph_path,
                graph_hash: &graph_hash,
                parent_id: Some(&input.base_version_id),
                semantics_json: semantics_json.as_deref(),
            },
            &input.base_version_id,
        )
        .await;

    match version_result {
        Ok(_) => {
            candidate.mark_committed().map_err(candidate_error)?;
            Ok(Json(workspace_state_value(&state, &workspace_id).await?))
        }
        Err(store_error) => Err(cleanup_ops_candidate(&state, &mut candidate, store_error).await),
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
                    size: None,
                    semantics: None,
                },
            })
        }
        ManualOpRequest::RemoveNode { id } => {
            ensure_non_empty("node id", &id)?;
            Ok(ProposalOp::RemoveNode { id })
        }
        ManualOpRequest::SetParam {
            id,
            key,
            prev,
            value,
        } => {
            ensure_non_empty("node id", &id)?;
            ensure_non_empty("param key", &key)?;
            let current_prev = base_graph
                .nodes
                .get(&id)
                .and_then(|node| node.params.as_object())
                .and_then(|params| params.get(&key))
                .cloned();
            Ok(ProposalOp::SetParam {
                id,
                key,
                prev: prev.or(current_prev),
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
        ManualOpRequest::ResizeNode { id, size } => {
            ensure_non_empty("node id", &id)?;
            ensure_finite_size(size)?;
            Ok(ProposalOp::ResizeNode { id, size })
        }
    }
}

fn op_error(index: usize, message: impl Into<String>) -> ApiError {
    ApiError::bad_request_with_details(message, json!({ "opIndex": index }))
}

fn graph_ops_error(err: GraphError, index: Option<usize>) -> ApiError {
    match err {
        GraphError::ProposalSuperseded { .. } => ApiError::conflict(err.to_string()),
        GraphError::SetParamConflict { .. } => {
            ApiError::conflict_with_details(err.to_string(), json!({ "opIndex": index }))
        }
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

fn ensure_finite_size(size: [f32; 2]) -> Result<(), ApiError> {
    if size.iter().any(|value| !value.is_finite()) || size[0] <= 0.0 || size[1] <= 0.0 {
        return Err(ApiError::bad_request(
            "node size must contain positive finite numbers",
        ));
    }
    Ok(())
}

fn ensure_idempotency_key(key: &str) -> Result<(), ApiError> {
    if key.trim().is_empty() {
        return Err(ApiError::bad_request("idempotencyKey must not be empty"));
    }
    if key.len() > 128 {
        return Err(ApiError::bad_request(
            "idempotencyKey must be 128 bytes or less",
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
        ProposalOp::ResizeNode { id, .. } => format!("Resize node {id}"),
        ProposalOp::SetSemantics { id, .. } => format!("Rebind node {id}"),
    }
}

pub(crate) fn ops_idempotency_digest(
    workspace_id: &str,
    base_version_id: &str,
    idempotency_key: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"helixflow:ops-idempotency:v1");
    for value in [workspace_id, base_version_id, idempotency_key] {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value.as_bytes());
    }
    hex::encode(hasher.finalize())
}

async fn accepted_idempotent_version(
    state: &AppState,
    workspace_id: &str,
    base_version_id: &str,
    graph_path: &str,
    graph_hash: &str,
) -> Result<bool, ApiError> {
    let references = state
        .store
        .version_file_references(graph_path)
        .await
        .map_err(ApiError::store)?;
    let [version] = references.as_slice() else {
        return Ok(false);
    };
    if version.workspace_id != workspace_id
        || version.parent_id.as_deref() != Some(base_version_id)
        || version.graph_path != graph_path
        || version.graph_hash != graph_hash
    {
        return Ok(false);
    }
    read_version_graph(&state.data_dir, version)
        .await
        .map_err(candidate_error)?;
    Ok(true)
}

fn is_publish_collision(error: &VersionFileConsistencyError) -> bool {
    matches!(
        error,
        VersionFileConsistencyError::Io {
            operation: "publish_candidate",
            kind: io::ErrorKind::AlreadyExists,
        }
    )
}

async fn cleanup_ops_candidate(
    state: &AppState,
    candidate: &mut VersionFileCandidate,
    store_error: StoreError,
) -> ApiError {
    match candidate.cleanup_after_store_error(&state.store).await {
        Ok(_) => match store_error {
            StoreError::VersionConflict { .. } | StoreError::PendingProposalConflict { .. } => {
                ApiError::conflict("workspace changed while applying manual ops; refresh and retry")
            }
            error => ApiError::store(error),
        },
        Err(cleanup_error) => ApiError::server_error(format!(
            "manual ops commit failed and candidate cleanup was deferred: {cleanup_error}"
        )),
    }
}

fn candidate_error(error: VersionFileConsistencyError) -> ApiError {
    ApiError::server_error(error.to_string())
}
