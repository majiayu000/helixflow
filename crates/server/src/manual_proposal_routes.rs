use std::path::PathBuf;

use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_graph::{
    GraphEdge, GraphError, GraphNode, GraphService, ProposalDraft, ProposalKind, ProposalOp,
    WorkflowGraph,
};
use helixflow_registry::NodeRegistry;
use helixflow_store::{NewMessage, NewProposal};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::{read_graph_file, write_json_file};
use crate::workbench_payload::proposal_payload_from_prepared;
use crate::workspace_state::workspace_state_value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ManualProposalRequest {
    pub(crate) base_version_id: String,
    pub(crate) title: Option<String>,
    pub(crate) summary: Option<String>,
    pub(crate) op: ManualProposalOpRequest,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ManualProposalOpRequest {
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
}

pub(crate) async fn create_manual_workspace_proposal(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
    Json(input): Json<ManualProposalRequest>,
) -> Result<Json<Value>, ApiError> {
    let workspace = state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    let current_version_id = workspace.cur_version_id.as_deref().ok_or_else(|| {
        ApiError::bad_request(format!(
            "workspace `{workspace_id}` has no current version for manual proposal"
        ))
    })?;
    if input.base_version_id != current_version_id {
        return Err(ApiError::conflict(format!(
            "manual proposal base `{}` is superseded by `{current_version_id}`",
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
        return Err(ApiError::bad_request(
            "workspace already has a pending proposal; apply or dismiss it first",
        ));
    }

    let current_version = state
        .store
        .version(current_version_id)
        .await
        .map_err(ApiError::store)?;
    let current_graph = read_graph_file(&state.data_dir, &current_version.graph_path).await?;
    let registry = NodeRegistry::builtin();
    let op = manual_op_to_proposal_op(input.op, &current_graph, &registry)?;
    let title = input
        .title
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| manual_title(&op));
    let summary = input
        .summary
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| manual_summary(&op));
    let draft = ProposalDraft {
        base_version_id: current_version_id.to_owned(),
        kind: ProposalKind::Modify,
        title,
        summary,
        ops: vec![op],
        message_id: None,
    };
    let prepared = GraphService::new(registry)
        .preview_proposal(&current_graph, current_version_id, draft)
        .map_err(manual_preview_error)?;
    let (ops_path, preview_path) = manual_proposal_storage_paths(&workspace_id);
    write_json_file(
        &state.data_dir,
        &ops_path,
        &prepared.ops,
        "write manual proposal ops",
    )
    .await?;
    write_json_file(
        &state.data_dir,
        &preview_path,
        &prepared.preview_graph,
        "write manual proposal preview graph",
    )
    .await?;
    let ops_path_string = ops_path.to_string_lossy().into_owned();
    let preview_path_string = preview_path.to_string_lossy().into_owned();
    let proposal_record = state
        .store
        .create_proposal(NewProposal {
            workspace_id: &workspace_id,
            base_version_id: &prepared.base_version_id,
            kind: "modify",
            title: &prepared.title,
            summary: &prepared.summary,
            ops_path: &ops_path_string,
            preview_graph_path: Some(&preview_path_string),
            message_id: None,
        })
        .await
        .map_err(ApiError::store)?;
    state
        .store
        .create_message(NewMessage {
            workspace_id: &workspace_id,
            role: "agent",
            kind: "proposal_pending",
            text: Some(&prepared.summary),
            ref_id: Some(&proposal_record.id),
            attachment_ids_json: None,
        })
        .await
        .map_err(ApiError::store)?;

    let mut body = workspace_state_value(&state, &workspace_id).await?;
    body["pendingProposal"] = serde_json::to_value(proposal_payload_from_prepared(
        proposal_record.id,
        &prepared,
    ))
    .map_err(|err| ApiError::server_error(format!("encode manual proposal: {err}")))?;
    Ok(Json(body))
}

fn manual_op_to_proposal_op(
    op: ManualProposalOpRequest,
    base_graph: &WorkflowGraph,
    registry: &NodeRegistry,
) -> Result<ProposalOp, ApiError> {
    match op {
        ManualProposalOpRequest::AddNode {
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
        ManualProposalOpRequest::RemoveNode { id } => {
            ensure_non_empty("node id", &id)?;
            Ok(ProposalOp::RemoveNode { id })
        }
        ManualProposalOpRequest::SetParam { id, key, value } => {
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
        ManualProposalOpRequest::AddEdge {
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
        ManualProposalOpRequest::RemoveEdge {
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

fn manual_summary(op: &ProposalOp) -> String {
    match op {
        ProposalOp::AddNode { node, .. } => format!("Manual proposal adds {}.", node.node_type),
        ProposalOp::RemoveNode { id } => format!("Manual proposal removes node `{id}`."),
        ProposalOp::SetParam { id, key, .. } => {
            format!("Manual proposal edits parameter `{key}` on node `{id}`.")
        }
        ProposalOp::AddEdge { edge } => format!(
            "Manual proposal connects `{}.{}` to `{}.{}`.",
            edge.from[0], edge.from[1], edge.to[0], edge.to[1]
        ),
        ProposalOp::RemoveEdge { edge } => format!(
            "Manual proposal disconnects `{}.{}` from `{}.{}`.",
            edge.from[0], edge.from[1], edge.to[0], edge.to[1]
        ),
        ProposalOp::MoveNode { id, .. } => format!("Manual proposal moves node `{id}`."),
    }
}

fn manual_proposal_storage_paths(workspace_id: &str) -> (PathBuf, PathBuf) {
    let request_id = Uuid::now_v7().simple().to_string();
    let proposal_dir = PathBuf::from("workspaces")
        .join(workspace_id)
        .join("proposals")
        .join(format!("manual-{request_id}"));
    (
        proposal_dir.join("ops.json"),
        proposal_dir.join("preview.json"),
    )
}

fn manual_preview_error(err: GraphError) -> ApiError {
    match err {
        GraphError::ProposalSuperseded { .. } => ApiError::conflict(err.to_string()),
        _ => ApiError::bad_request(err.to_string()),
    }
}
