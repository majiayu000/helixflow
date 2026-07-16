use std::path::PathBuf;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::{read_graph_file, write_json_file};
use crate::proposal_routes::graph_apply_error;
use helixflow_agent::ValidatedAgentProposal;
use helixflow_graph::{GraphService, ProposalKind};
use helixflow_registry::NodeRegistry;
use helixflow_store::{
    AutoApplyProposalVersionRecord, MessageRecord, NewProposal, NewVersion, StoreError,
    VersionSource,
};

pub(crate) async fn persist_and_apply_agent_proposal(
    state: &AppState,
    workspace_id: &str,
    proposal: &ValidatedAgentProposal,
) -> Result<MessageRecord, ApiError> {
    let (ops_path, preview_graph_path, applied_graph_path) =
        proposal_storage_paths(workspace_id, &proposal.session_id);
    write_json_file(
        &state.data_dir,
        &ops_path,
        &proposal.proposal.ops,
        "write proposal ops",
    )
    .await?;
    write_json_file(
        &state.data_dir,
        &preview_graph_path,
        &proposal.proposal.preview_graph,
        "write proposal preview graph",
    )
    .await?;
    let workspace = state
        .store
        .workspace(workspace_id)
        .await
        .map_err(ApiError::store)?;
    let current_version_id = workspace.cur_version_id.as_deref().ok_or_else(|| {
        ApiError::bad_request(format!(
            "workspace `{workspace_id}` has no current version for proposal apply"
        ))
    })?;
    let current_version = state
        .store
        .version(current_version_id)
        .await
        .map_err(ApiError::store)?;
    let current_graph = read_graph_file(&state.data_dir, &current_version.graph_path).await?;
    let applied_graph = GraphService::new(NodeRegistry::builtin())
        .apply_proposal(&current_graph, current_version_id, &proposal.proposal)
        .map_err(graph_apply_error)?;
    let graph_hash = write_json_file(
        &state.data_dir,
        &applied_graph_path,
        &applied_graph,
        "write auto-applied proposal graph",
    )
    .await?;

    let ops_path_string = ops_path.to_string_lossy().into_owned();
    let preview_graph_path_string = preview_graph_path.to_string_lossy().into_owned();
    let applied_graph_path_string = applied_graph_path.to_string_lossy().into_owned();
    let version_label = format!("Agent edit: {}", proposal.proposal.title);
    let message_text = format!(
        "已自动应用 `{}`。可在版本历史中回退。",
        proposal.proposal.title
    );
    let result = state
        .store
        .auto_apply_proposal_version(AutoApplyProposalVersionRecord {
            proposal: NewProposal {
                workspace_id,
                base_version_id: &proposal.proposal.base_version_id,
                kind: proposal_kind_as_str(proposal.proposal.kind),
                title: &proposal.proposal.title,
                summary: &proposal.proposal.summary,
                ops_path: &ops_path_string,
                preview_graph_path: Some(&preview_graph_path_string),
                message_id: None,
            },
            version: NewVersion {
                workspace_id,
                label: &version_label,
                source: VersionSource::Proposal,
                graph_path: &applied_graph_path_string,
                graph_hash: &graph_hash,
                parent_id: Some(&proposal.proposal.base_version_id),
            },
            message_text: &message_text,
        })
        .await
        .map_err(|err| match err {
            StoreError::VersionConflict { .. } => ApiError::conflict(err.to_string()),
            other => ApiError::store(other),
        })?;
    Ok(result.message)
}

fn proposal_storage_paths(workspace_id: &str, session_id: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir = PathBuf::from("workspaces")
        .join(workspace_id)
        .join("proposals")
        .join(session_id);
    (
        dir.join("ops.json"),
        dir.join("preview.json"),
        dir.join("applied.json"),
    )
}

pub(crate) fn proposal_kind_as_str(kind: ProposalKind) -> &'static str {
    match kind {
        ProposalKind::Create => "create",
        ProposalKind::Modify => "modify",
        ProposalKind::Fix => "fix",
        ProposalKind::Sweep => "sweep",
    }
}
