use std::path::PathBuf;

use helixflow_agent::ValidatedAgentProposal;
use helixflow_graph::{GraphService, PreparedProposal, ProposalKind};
use helixflow_registry::NodeRegistry;
use helixflow_store::{
    ApplyProposalVersionRecord, MessageRecord, NewMessage, NewProposal, NewVersion, ProposalRecord,
    VersionRecord, VersionSource,
};
use serde_json::json;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::{read_graph_file, write_json_file};

pub(crate) async fn persist_and_apply_agent_proposal(
    state: &AppState,
    workspace_id: &str,
    proposal: &ValidatedAgentProposal,
) -> Result<MessageRecord, ApiError> {
    let (ops_path, preview_graph_path) = proposal_storage_paths(workspace_id, &proposal.session_id);
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
    let ops_path_string = ops_path.to_string_lossy().into_owned();
    let preview_graph_path_string = preview_graph_path.to_string_lossy().into_owned();
    let proposal_record = state
        .store
        .create_proposal(NewProposal {
            workspace_id,
            base_version_id: &proposal.proposal.base_version_id,
            kind: proposal_kind_as_str(proposal.proposal.kind),
            title: &proposal.proposal.title,
            summary: &proposal.proposal.summary,
            ops_path: &ops_path_string,
            preview_graph_path: Some(&preview_graph_path_string),
            message_id: None,
        })
        .await
        .map_err(ApiError::store)?;
    let version =
        apply_agent_proposal(state, workspace_id, &proposal_record, &proposal.proposal).await?;
    let message = state
        .store
        .create_message(NewMessage {
            workspace_id,
            role: "agent",
            kind: "proposal_applied",
            text: Some(&format!(
                "已自动应用 `{}` 到版本 `{}`。可在历史记录中回退。",
                proposal.proposal.title, version.id
            )),
            ref_id: Some(&proposal_record.id),
            attachment_ids_json: Some(&json!({ "versionId": version.id }).to_string()),
        })
        .await
        .map_err(ApiError::store)?;
    state
        .store
        .attach_proposal_message(&proposal_record.id, &message.id)
        .await
        .map_err(ApiError::store)?;

    Ok(message)
}

async fn apply_agent_proposal(
    state: &AppState,
    workspace_id: &str,
    proposal_record: &ProposalRecord,
    prepared: &PreparedProposal,
) -> Result<VersionRecord, ApiError> {
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
        .apply_proposal(&current_graph, current_version_id, prepared)
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    let graph_path = applied_graph_path(proposal_record);
    let graph_hash = write_json_file(
        &state.data_dir,
        &graph_path,
        &applied_graph,
        "write auto-applied proposal graph",
    )
    .await?;
    let graph_path_string = graph_path.to_string_lossy().into_owned();
    let version_label = format!("Agent edit: {}", proposal_record.title);

    state
        .store
        .create_version_after_applying_proposal(ApplyProposalVersionRecord {
            proposal_id: &proposal_record.id,
            expected_current_version_id: current_version_id,
            version: NewVersion {
                workspace_id,
                label: &version_label,
                source: VersionSource::Proposal,
                graph_path: &graph_path_string,
                graph_hash: &graph_hash,
                parent_id: Some(&proposal_record.base_version_id),
            },
        })
        .await
        .map_err(ApiError::store)
}

fn proposal_storage_paths(workspace_id: &str, session_id: &str) -> (PathBuf, PathBuf) {
    let dir = PathBuf::from("workspaces")
        .join(workspace_id)
        .join("proposals")
        .join(session_id);
    (dir.join("ops.json"), dir.join("preview.json"))
}

pub(crate) fn applied_graph_path(proposal: &ProposalRecord) -> PathBuf {
    PathBuf::from("workspaces")
        .join(&proposal.workspace_id)
        .join("graphs")
        .join(format!("{}.json", proposal.id))
}

pub(crate) fn proposal_kind_as_str(kind: ProposalKind) -> &'static str {
    match kind {
        ProposalKind::Create => "create",
        ProposalKind::Modify => "modify",
        ProposalKind::Fix => "fix",
        ProposalKind::Sweep => "sweep",
    }
}
