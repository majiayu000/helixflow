use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::proposal_routes::graph_apply_error;
use crate::version_commit::{VersionCommitError, commit_version_candidates};
use crate::version_file_consistency::{
    CandidateKind, VersionFileCandidate, VersionFileCandidateSet, VersionFileConsistencyError,
    read_version_graph,
};
use crate::version_semantics::derive_semantics_json;
use helixflow_graph::{GraphService, PreparedProposal, ProposalKind};
use helixflow_registry::NodeRegistry;
use helixflow_store::{
    AutoApplyProposalVersionRecord, CompleteAgentContractObservation, MessageRecord, NewProposal,
    NewVersion, VersionRecord, VersionSource,
};

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct AutoApplyCommitHook {
    published: std::sync::Arc<tokio::sync::Barrier>,
    release: std::sync::Arc<tokio::sync::Barrier>,
}

struct PreparedAgentApply {
    candidates: VersionFileCandidateSet,
    ops_path: String,
    preview_path: String,
    applied_path: String,
    applied_hash: String,
    semantics_json: Option<String>,
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

#[cfg(test)]
pub(crate) async fn persist_and_apply_agent_proposal(
    state: &AppState,
    workspace_id: &str,
    proposal: &PreparedProposal,
    semantics_json: Option<&str>,
) -> Result<MessageRecord, ApiError> {
    #[cfg(test)]
    {
        persist_and_apply_agent_proposal_inner(
            state,
            workspace_id,
            proposal,
            semantics_json,
            None,
            None,
            None,
        )
        .await
    }
    #[cfg(not(test))]
    {
        persist_and_apply_agent_proposal_inner(
            state,
            workspace_id,
            proposal,
            semantics_json,
            None,
            None,
        )
        .await
    }
}

pub(crate) async fn persist_and_apply_agent_proposal_observed(
    state: &AppState,
    workspace_id: &str,
    proposal: &PreparedProposal,
    semantics_json: Option<&str>,
    completion: CompleteAgentContractObservation<'_>,
    conversation_id: &str,
    turn_id: &str,
) -> Result<MessageRecord, ApiError> {
    #[cfg(test)]
    {
        persist_and_apply_agent_proposal_inner(
            state,
            workspace_id,
            proposal,
            semantics_json,
            Some(completion),
            Some((conversation_id, turn_id)),
            None,
        )
        .await
    }
    #[cfg(not(test))]
    {
        persist_and_apply_agent_proposal_inner(
            state,
            workspace_id,
            proposal,
            semantics_json,
            Some(completion),
            Some((conversation_id, turn_id)),
        )
        .await
    }
}

#[cfg(test)]
pub(crate) async fn persist_and_apply_agent_proposal_with_hook(
    state: &AppState,
    workspace_id: &str,
    proposal: &PreparedProposal,
    hook: &AutoApplyCommitHook,
) -> Result<MessageRecord, ApiError> {
    persist_and_apply_agent_proposal_inner(
        state,
        workspace_id,
        proposal,
        None,
        None,
        None,
        Some(hook),
    )
    .await
}

async fn persist_and_apply_agent_proposal_inner(
    state: &AppState,
    workspace_id: &str,
    proposal: &PreparedProposal,
    semantics_json: Option<&str>,
    observation_completion: Option<CompleteAgentContractObservation<'_>>,
    turn_context: Option<(&str, &str)>,
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
    if proposal.base_version_id != current_version_id {
        return Err(ApiError::conflict(format!(
            "proposal base `{}` is superseded by `{current_version_id}`",
            proposal.base_version_id
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
    let mut prepared = prepare_agent_apply(
        workspace_id,
        &current_version,
        &current_graph,
        proposal,
        semantics_json,
    )?;
    let semantics_json = prepared.semantics_json.as_deref();
    let mut candidates = std::mem::take(&mut prepared.candidates);
    let version_label = format!("Agent edit: {}", proposal.title);
    let message_text = format!("已自动应用 `{}`。可在版本历史中回退。", proposal.title);
    let auto_apply = AutoApplyProposalVersionRecord {
        proposal: NewProposal {
            workspace_id,
            base_version_id: &proposal.base_version_id,
            kind: proposal_kind_as_str(proposal.kind),
            title: &proposal.title,
            summary: &proposal.summary,
            ops_path: &prepared.ops_path,
            preview_graph_path: Some(&prepared.preview_path),
            message_id: None,
        },
        version: NewVersion {
            workspace_id,
            label: &version_label,
            source: VersionSource::Proposal,
            graph_path: &prepared.applied_path,
            graph_hash: &prepared.applied_hash,
            parent_id: Some(&proposal.base_version_id),
            semantics_json,
        },
        message_text: &message_text,
    };
    let transaction = async {
        #[cfg(test)]
        if let Some(hook) = commit_hook {
            hook.after_publish().await;
        }
        match observation_completion {
            Some(completion) => state
                .store
                .auto_apply_proposal_version_with_observation_and_turn(
                    auto_apply,
                    completion,
                    turn_context
                        .map(|(conversation_id, _)| conversation_id)
                        .ok_or_else(|| {
                            ApiError::server_error(
                                "observed proposal is missing conversation context",
                            )
                        })?,
                    turn_context.map(|(_, turn_id)| turn_id).ok_or_else(|| {
                        ApiError::server_error("observed proposal is missing turn context")
                    })?,
                )
                .await
                .map_err(ApiError::store),
            None => state
                .store
                .auto_apply_proposal_version(auto_apply)
                .await
                .map_err(ApiError::store),
        }
    };
    commit_version_candidates(&mut candidates, &state.data_dir, &state.store, transaction)
        .await
        .map(|result| result.message)
        .map_err(auto_apply_commit_error)
}

fn prepare_agent_apply(
    workspace_id: &str,
    current_version: &VersionRecord,
    current_graph: &helixflow_graph::WorkflowGraph,
    proposal: &PreparedProposal,
    semantics_json: Option<&str>,
) -> Result<PreparedAgentApply, ApiError> {
    let applied_graph = GraphService::new(NodeRegistry::builtin())
        .apply_proposal(current_graph, &current_version.id, proposal)
        .map_err(graph_apply_error)?;
    let semantics_json = match semantics_json {
        Some(value) => Some(value.to_owned()),
        None => derive_semantics_json(current_version, current_graph, &applied_graph)?,
    };
    let ops_candidate =
        VersionFileCandidate::from_json(workspace_id, CandidateKind::ProposalOps, &proposal.ops)
            .map_err(candidate_error)?;
    let ops_path = ops_candidate
        .relative_path_text()
        .map_err(candidate_error)?
        .to_owned();
    let preview_candidate = VersionFileCandidate::from_json(
        workspace_id,
        CandidateKind::ProposalPreview,
        &proposal.preview_graph,
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
    Ok(PreparedAgentApply {
        candidates,
        ops_path,
        preview_path,
        applied_path,
        applied_hash,
        semantics_json,
    })
}

fn auto_apply_commit_error(error: VersionCommitError<ApiError>) -> ApiError {
    match error {
        VersionCommitError::Consistency(error) => candidate_error(error),
        VersionCommitError::Store {
            source,
            cleanup: None,
        } => source,
        VersionCommitError::Store {
            cleanup: Some(cleanup_error),
            ..
        } => ApiError::server_error(format!(
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
