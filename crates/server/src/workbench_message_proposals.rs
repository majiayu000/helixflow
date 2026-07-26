use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::proposal_routes::graph_apply_error;
use crate::version_file_consistency::{
    CandidateKind, VersionFileCandidate, VersionFileCandidateSet, VersionFileConsistencyError,
    read_version_graph,
};
use helixflow_agent::ValidatedAgentProposal;
use helixflow_graph::{GraphService, ProposalKind};
use helixflow_registry::NodeRegistry;
use helixflow_store::{
    AutoApplyProposalVersionRecord, MessageRecord, NewProposal, NewVersion, StoreError,
    VersionSource,
};

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct AutoApplyCommitHook {
    published: std::sync::Arc<tokio::sync::Barrier>,
    release: std::sync::Arc<tokio::sync::Barrier>,
}

#[cfg(test)]
impl AutoApplyCommitHook {
    pub(crate) fn new() -> Self {
        Self {
            published: std::sync::Arc::new(tokio::sync::Barrier::new(2)),
            release: std::sync::Arc::new(tokio::sync::Barrier::new(2)),
        }
    }

    pub(crate) async fn wait_until_published(&self) {
        self.published.wait().await;
    }

    pub(crate) async fn release_store(&self) {
        self.release.wait().await;
    }

    async fn after_publish(&self) {
        self.published.wait().await;
        self.release.wait().await;
    }
}

pub(crate) async fn persist_and_apply_agent_proposal(
    state: &AppState,
    workspace_id: &str,
    proposal: &ValidatedAgentProposal,
    semantics_json: Option<&str>,
) -> Result<MessageRecord, ApiError> {
    #[cfg(test)]
    {
        persist_and_apply_agent_proposal_inner(state, workspace_id, proposal, semantics_json, None)
            .await
    }
    #[cfg(not(test))]
    {
        persist_and_apply_agent_proposal_inner(state, workspace_id, proposal, semantics_json).await
    }
}

#[cfg(test)]
pub(crate) async fn persist_and_apply_agent_proposal_with_hook(
    state: &AppState,
    workspace_id: &str,
    proposal: &ValidatedAgentProposal,
    hook: &AutoApplyCommitHook,
) -> Result<MessageRecord, ApiError> {
    persist_and_apply_agent_proposal_inner(state, workspace_id, proposal, None, Some(hook)).await
}

async fn persist_and_apply_agent_proposal_inner(
    state: &AppState,
    workspace_id: &str,
    proposal: &ValidatedAgentProposal,
    semantics_json: Option<&str>,
    #[cfg(test)] commit_hook: Option<&AutoApplyCommitHook>,
) -> Result<MessageRecord, ApiError> {
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
    if proposal.proposal.base_version_id != current_version_id {
        return Err(ApiError::conflict(format!(
            "proposal base `{}` is superseded by `{current_version_id}`",
            proposal.proposal.base_version_id
        )));
    }
    let current_version = state
        .store
        .version(current_version_id)
        .await
        .map_err(ApiError::store)?;
    let current_graph = read_version_graph(&state.data_dir, &current_version)
        .await
        .map_err(candidate_error)?;
    let applied_graph = GraphService::new(NodeRegistry::builtin())
        .apply_proposal(&current_graph, current_version_id, &proposal.proposal)
        .map_err(graph_apply_error)?;
    let ops_candidate = VersionFileCandidate::from_json(
        workspace_id,
        CandidateKind::ProposalOps,
        &proposal.proposal.ops,
    )
    .map_err(candidate_error)?;
    let ops_path = ops_candidate
        .relative_path_text()
        .map_err(candidate_error)?
        .to_owned();
    let preview_candidate = VersionFileCandidate::from_json(
        workspace_id,
        CandidateKind::ProposalPreview,
        &proposal.proposal.preview_graph,
    )
    .map_err(candidate_error)?;
    let preview_path = preview_candidate
        .relative_path_text()
        .map_err(candidate_error)?
        .to_owned();
    let applied_candidate = VersionFileCandidate::from_graph(
        workspace_id,
        CandidateKind::ProposalApplied,
        &applied_graph,
    )
    .map_err(candidate_error)?;
    let applied_path = applied_candidate
        .relative_path_text()
        .map_err(candidate_error)?
        .to_owned();
    let applied_hash = applied_candidate.graph_hash().to_owned();
    let mut candidates = VersionFileCandidateSet::default();
    candidates.push(ops_candidate);
    candidates.push(preview_candidate);
    candidates.push(applied_candidate);
    candidates
        .publish_all(&state.data_dir)
        .map_err(candidate_error)?;
    #[cfg(test)]
    if let Some(hook) = commit_hook {
        hook.after_publish().await;
    }

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
                ops_path: &ops_path,
                preview_graph_path: Some(&preview_path),
                message_id: None,
            },
            version: NewVersion {
                workspace_id,
                label: &version_label,
                source: VersionSource::Proposal,
                graph_path: &applied_path,
                graph_hash: &applied_hash,
                parent_id: Some(&proposal.proposal.base_version_id),
                semantics_json,
            },
            message_text: &message_text,
        })
        .await;
    match result {
        Ok(result) => {
            candidates.mark_all_committed().map_err(candidate_error)?;
            Ok(result.message)
        }
        Err(store_error) => Err(cleanup_auto_candidates(state, &mut candidates, store_error).await),
    }
}

async fn cleanup_auto_candidates(
    state: &AppState,
    candidates: &mut VersionFileCandidateSet,
    store_error: StoreError,
) -> ApiError {
    match candidates.cleanup_after_store_error(&state.store).await {
        Ok(_) => match store_error {
            StoreError::VersionConflict { .. } | StoreError::VersionParentMismatch { .. } => {
                ApiError::conflict("workspace changed while applying agent proposal")
            }
            error => ApiError::store(error),
        },
        Err(cleanup_error) => ApiError::server_error(format!(
            "agent proposal commit failed and candidate cleanup was deferred: {cleanup_error}"
        )),
    }
}

fn candidate_error(error: VersionFileConsistencyError) -> ApiError {
    ApiError::server_error(error.to_string())
}

pub(crate) fn proposal_kind_as_str(kind: ProposalKind) -> &'static str {
    match kind {
        ProposalKind::Create => "create",
        ProposalKind::Modify => "modify",
        ProposalKind::Fix => "fix",
        ProposalKind::Sweep => "sweep",
    }
}
