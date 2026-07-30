use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::proposal_routes::graph_apply_error;
use crate::version_file_consistency::{
    CandidateKind, VersionFileCandidate, VersionFileCandidateSet, VersionFileConsistencyError,
    read_version_graph,
};
use crate::version_semantics::derive_semantics_json;
use helixflow_agent::ValidatedAgentProposal;
use helixflow_gateway::Provider;
use helixflow_graph::{GraphService, ProposalKind};
use helixflow_registry::NodeRegistry;
use helixflow_store::{
    ApplyRunFixVersionRecord, AutoApplyProposalVersionRecord, CompleteAgentContractObservation,
    MessageRecord, NewProposal, NewVersion, RunFixAttemptRecord, StoreError, VersionRecord,
    VersionSource,
};

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct AutoApplyCommitHook {
    published: std::sync::Arc<tokio::sync::Barrier>,
    release: std::sync::Arc<tokio::sync::Barrier>,
}

struct PreparedAgentApply {
    applied_graph: helixflow_graph::WorkflowGraph,
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
    proposal: &ValidatedAgentProposal,
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
        )
        .await
    }
    #[cfg(not(test))]
    {
        persist_and_apply_agent_proposal_inner(state, workspace_id, proposal, semantics_json, None)
            .await
    }
}

pub(crate) async fn persist_and_apply_agent_proposal_observed(
    state: &AppState,
    workspace_id: &str,
    proposal: &ValidatedAgentProposal,
    semantics_json: Option<&str>,
    completion: CompleteAgentContractObservation<'_>,
) -> Result<MessageRecord, ApiError> {
    #[cfg(test)]
    {
        persist_and_apply_agent_proposal_inner(
            state,
            workspace_id,
            proposal,
            semantics_json,
            Some(completion),
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
        )
        .await
    }
}

#[cfg(test)]
pub(crate) async fn persist_and_apply_agent_proposal_with_hook(
    state: &AppState,
    workspace_id: &str,
    proposal: &ValidatedAgentProposal,
    hook: &AutoApplyCommitHook,
) -> Result<MessageRecord, ApiError> {
    persist_and_apply_agent_proposal_inner(state, workspace_id, proposal, None, None, Some(hook))
        .await
}

async fn persist_and_apply_agent_proposal_inner(
    state: &AppState,
    workspace_id: &str,
    proposal: &ValidatedAgentProposal,
    semantics_json: Option<&str>,
    observation_completion: Option<CompleteAgentContractObservation<'_>>,
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
    let mut prepared = prepare_agent_apply(
        workspace_id,
        &current_version,
        &current_graph,
        proposal,
        semantics_json,
    )?;
    let semantics_json = prepared.semantics_json.as_deref();
    let mut candidates = std::mem::take(&mut prepared.candidates);
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
    let auto_apply = AutoApplyProposalVersionRecord {
        proposal: NewProposal {
            workspace_id,
            base_version_id: &proposal.proposal.base_version_id,
            kind: proposal_kind_as_str(proposal.proposal.kind),
            title: &proposal.proposal.title,
            summary: &proposal.proposal.summary,
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
            parent_id: Some(&proposal.proposal.base_version_id),
            semantics_json,
        },
        message_text: &message_text,
    };
    let result = match observation_completion {
        Some(completion) => {
            state
                .store
                .auto_apply_proposal_version_with_observation(auto_apply, completion)
                .await
        }
        None => state.store.auto_apply_proposal_version(auto_apply).await,
    };
    match result {
        Ok(result) => {
            candidates.mark_all_committed().map_err(candidate_error)?;
            Ok(result.message)
        }
        Err(store_error) => Err(cleanup_auto_candidates(state, &mut candidates, store_error).await),
    }
}

pub(crate) async fn persist_and_apply_run_fix_proposal(
    state: &AppState,
    attempt: &RunFixAttemptRecord,
    proposal: &ValidatedAgentProposal,
    semantics_json: Option<&str>,
) -> Result<(VersionRecord, helixflow_graph::WorkflowGraph), ApiError> {
    if proposal.proposal.kind != ProposalKind::Fix
        || proposal.proposal.base_version_id != attempt.source_version_id
    {
        return Err(ApiError::conflict(
            "agent fix proposal has an invalid base or kind",
        ));
    }
    let current_version = state
        .store
        .version(&attempt.source_version_id)
        .await
        .map_err(ApiError::store)?;
    let current_graph = read_version_graph(&state.data_dir, &current_version)
        .await
        .map_err(candidate_error)?;
    let mut prepared = prepare_agent_apply(
        &attempt.workspace_id,
        &current_version,
        &current_graph,
        proposal,
        semantics_json,
    )?;
    prepared
        .candidates
        .publish_all(&state.data_dir)
        .map_err(candidate_error)?;
    let workspace = state
        .store
        .workspace(&attempt.workspace_id)
        .await
        .map_err(ApiError::store)?;
    let effective_provider = state.selected_provider_for_workspace(&workspace);
    let scope = state
        .provider_registry
        .recovery_scope_fingerprint(&effective_provider);
    let catalog =
        helixflow_run::provider_catalog_fingerprint(&state.provider_registry, &effective_provider);
    let version_label = format!("Agent fix: {}", proposal.proposal.title);
    let message_text = format!(
        "已自动应用修复 `{}`；新的 provider run 仍需通过成本门。",
        proposal.proposal.title
    );
    let result = state
        .store
        .apply_run_fix_version(ApplyRunFixVersionRecord {
            attempt_id: &attempt.id,
            proposal: NewProposal {
                workspace_id: &attempt.workspace_id,
                base_version_id: &attempt.source_version_id,
                kind: "fix",
                title: &proposal.proposal.title,
                summary: &proposal.proposal.summary,
                ops_path: &prepared.ops_path,
                preview_graph_path: Some(&prepared.preview_path),
                message_id: None,
            },
            version: NewVersion {
                workspace_id: &attempt.workspace_id,
                label: &version_label,
                source: VersionSource::Proposal,
                graph_path: &prepared.applied_path,
                graph_hash: &prepared.applied_hash,
                parent_id: Some(&attempt.source_version_id),
                semantics_json: prepared.semantics_json.as_deref(),
            },
            message_text: &message_text,
            actual_effective_provider_id: &effective_provider,
            actual_recovery_scope_fingerprint: &scope,
            actual_provider_catalog_fingerprint: &catalog,
        })
        .await;
    match result {
        Ok(result) => {
            prepared
                .candidates
                .mark_all_committed()
                .map_err(candidate_error)?;
            Ok((result.applied.version, prepared.applied_graph))
        }
        Err(error) => Err(cleanup_auto_candidates(state, &mut prepared.candidates, error).await),
    }
}

fn prepare_agent_apply(
    workspace_id: &str,
    current_version: &VersionRecord,
    current_graph: &helixflow_graph::WorkflowGraph,
    proposal: &ValidatedAgentProposal,
    semantics_json: Option<&str>,
) -> Result<PreparedAgentApply, ApiError> {
    let applied_graph = GraphService::new(NodeRegistry::builtin())
        .apply_proposal(current_graph, &current_version.id, &proposal.proposal)
        .map_err(graph_apply_error)?;
    let semantics_json = match semantics_json {
        Some(value) => Some(value.to_owned()),
        None => derive_semantics_json(current_version, current_graph, &applied_graph)?,
    };
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
    Ok(PreparedAgentApply {
        applied_graph,
        candidates,
        ops_path,
        preview_path,
        applied_path,
        applied_hash,
        semantics_json,
    })
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
