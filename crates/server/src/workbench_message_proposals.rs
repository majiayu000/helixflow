use std::collections::BTreeSet;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::catalog_routes::connector_availability;
use crate::proposal_routes::graph_apply_error;
use crate::version_file_consistency::{
    CandidateKind, VersionFileCandidate, VersionFileCandidateSet, VersionFileConsistencyError,
    read_version_graph,
};
use crate::version_semantics::derive_semantics_json;
use crate::workbench_message::{
    ensure_fix_scope, format_exact_debug_run_context, persist_agent_logs, safe_error_summary,
    safe_graph_projection,
};
use helixflow_agent::{AgentSessionRequest, AgentSkill, TurnMode, ValidatedAgentProposal};
use helixflow_compiler::{CompileOutcome, compile};
use helixflow_gateway::Provider;
use helixflow_graph::{GraphService, ProposalDraft, ProposalKind, WorkflowGraph};
use helixflow_registry::NodeRegistry;
use helixflow_store::{
    ApplyRunFixVersionRecord, AutoApplyProposalVersionRecord, MessageRecord, NewProposal,
    NewVersion, RunFixAttemptRecord, RunFixClaim, RunFixSnapshot, StoreError, VersionRecord,
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
    let result = state
        .store
        .auto_apply_proposal_version(AutoApplyProposalVersionRecord {
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

pub(crate) async fn run_agent_fix_worker(state: AppState) {
    let mut events = state.events.subscribe();
    if let Err(error) = recover_run_agent_fixes_once(&state).await {
        eprintln!("run agent fix startup recovery failed: {}", error.message);
    }
    loop {
        match events.recv().await {
            Ok(event) if event.ev == "run.failed" => {
                if let Err(error) = recover_run_agent_fixes_once(&state).await {
                    eprintln!("run agent fix coordinator failed: {}", error.message);
                }
            }
            Ok(_) => {}
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                if let Err(error) = recover_run_agent_fixes_once(&state).await {
                    eprintln!("run agent fix lag recovery failed: {}", error.message);
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
        }
    }
}

pub(crate) async fn recover_run_agent_fixes_once(state: &AppState) -> Result<(), ApiError> {
    let policy = match helixflow_run::agent_fix_policy() {
        Ok(policy) if !policy.enabled => return Ok(()),
        Ok(policy) => policy,
        Err(_) => {
            for attempt in state
                .store
                .unfinished_run_fix_attempts()
                .await
                .map_err(ApiError::store)?
            {
                fail_fix_attempt(state, &attempt, "FIX_INVALID_CONFIGURATION", None).await?;
            }
            for source_run_id in state
                .store
                .pending_run_fix_source_ids()
                .await
                .map_err(ApiError::store)?
            {
                state
                    .store
                    .exhaust_run_fix_without_attempt(&source_run_id, 0, "FIX_INVALID_CONFIGURATION")
                    .await
                    .map_err(ApiError::store)?;
            }
            drain_run_fix_outbox(state).await?;
            return Ok(());
        }
    };
    recover_run_agent_fixes_with_policy(state, policy).await
}

pub(crate) async fn recover_run_agent_fixes_with_policy(
    state: &AppState,
    policy: helixflow_run::AgentFixPolicy,
) -> Result<(), ApiError> {
    for attempt in state
        .store
        .unfinished_run_fix_attempts()
        .await
        .map_err(ApiError::store)?
    {
        match attempt.state.as_str() {
            "claimed" | "agent_running" => {
                fail_fix_attempt(state, &attempt, "FIX_RECOVERY_INTERRUPTED", None).await?;
            }
            "version_applied" | "child_preparing" => {
                resume_fix_child(state, &attempt).await?;
            }
            _ => {}
        }
    }
    for source_run_id in state
        .store
        .pending_run_fix_source_ids()
        .await
        .map_err(ApiError::store)?
    {
        process_fix_source(state, &source_run_id, policy.max_attempts).await?;
    }
    drain_run_fix_outbox(state).await
}

async fn process_fix_source(
    state: &AppState,
    source_run_id: &str,
    max_attempts: u32,
) -> Result<(), ApiError> {
    loop {
        let source = state
            .store
            .run(source_run_id)
            .await
            .map_err(ApiError::store)?;
        let workspace = state
            .store
            .workspace(&source.workspace_id)
            .await
            .map_err(ApiError::store)?;
        if source.status != "failed"
            || workspace.cur_version_id.as_deref() != Some(source.version_id.as_str())
        {
            state
                .store
                .exhaust_run_fix_without_attempt(source_run_id, max_attempts, "FIX_SOURCE_CHANGED")
                .await
                .map_err(ApiError::store)?;
            return Ok(());
        }
        let effective_provider = state.selected_provider_for_workspace(&workspace);
        let scope = state
            .provider_registry
            .recovery_scope_fingerprint(&effective_provider);
        let catalog = helixflow_run::provider_catalog_fingerprint(
            &state.provider_registry,
            &effective_provider,
        );
        let claim = state
            .store
            .claim_run_fix_attempt(
                source_run_id,
                max_attempts,
                RunFixSnapshot {
                    expected_runtime_provider_id: workspace.runtime_provider_id.as_deref(),
                    effective_provider_id: &effective_provider,
                    expected_recovery_scope_fingerprint: &scope,
                    provider_catalog_fingerprint: &catalog,
                },
            )
            .await
            .map_err(ApiError::store)?;
        drain_run_fix_outbox(state).await?;
        let RunFixClaim::Claimed(attempt) = claim else {
            return Ok(());
        };
        state
            .store
            .mark_run_fix_agent_running(&attempt.id)
            .await
            .map_err(ApiError::store)?;
        match execute_fix_attempt(state, &attempt).await {
            Ok(()) => return drain_run_fix_outbox(state).await,
            Err((reason_code, summary)) => {
                fail_fix_attempt(state, &attempt, reason_code, summary.as_deref()).await?;
                drain_run_fix_outbox(state).await?;
            }
        }
    }
}

async fn execute_fix_attempt(
    state: &AppState,
    attempt: &RunFixAttemptRecord,
) -> Result<(), (&'static str, Option<String>)> {
    let source = state
        .store
        .run(&attempt.source_run_id)
        .await
        .map_err(|error| fix_failure("FIX_SOURCE_CHANGED", error.to_string()))?;
    let steps = state
        .store
        .run_steps(&source.id)
        .await
        .map_err(|error| fix_failure("FIX_SOURCE_CHANGED", error.to_string()))?;
    let version = state
        .store
        .version(&attempt.source_version_id)
        .await
        .map_err(|error| fix_failure("FIX_SOURCE_CHANGED", error.to_string()))?;
    let graph = read_version_graph(&state.data_dir, &version)
        .await
        .map_err(|error| fix_failure("FIX_SOURCE_CHANGED", error.to_string()))?;
    let workspace = state
        .store
        .workspace(&attempt.workspace_id)
        .await
        .map_err(|error| fix_failure("FIX_PROVIDER_CHANGED", error.to_string()))?;
    let request = AgentSessionRequest {
        workspace_id: attempt.workspace_id.clone(),
        base_version_id: attempt.source_version_id.clone(),
        user_message: "修复失败节点及其依赖闭包；不得改动无关节点、workspace 或 provider 设置。"
            .to_owned(),
        history: Vec::new(),
        graph: safe_graph_projection(&graph),
        provider_catalog: state.provider_catalog_for_workspace(&workspace),
        run_context: Some(format_exact_debug_run_context(&source, &steps)),
        sessions_dir: state.agent_sessions_dir.clone(),
        mode: TurnMode::DebugWorkflow,
        skill: AgentSkill::FixError,
        canvas_context: None,
        use_intent_contract: state.use_intent_contract,
    };
    let (proposal, semantics_json) = prepare_fix_proposal(state, request, &graph).await?;
    persist_agent_logs(
        state,
        &attempt.workspace_id,
        &proposal.session_id,
        &proposal.agent_logs,
    )
    .await
    .map_err(|error| fix_failure("FIX_AGENT_FAILED", error.message))?;
    let failed_node_ids = steps
        .iter()
        .filter(|step| step.state == "failed")
        .map(|step| step.node_id.clone())
        .collect::<BTreeSet<_>>();
    ensure_fix_scope(&graph, &failed_node_ids, &proposal.proposal).map_err(|code| (code, None))?;
    let (_, target_graph) =
        persist_and_apply_run_fix_proposal(state, attempt, &proposal, semantics_json.as_deref())
            .await
            .map_err(|error| fix_failure("FIX_VERSION_CONFLICT", error.message))?;
    state
        .runner
        .prepare_run_fix_child(&attempt.id, target_graph)
        .await
        .map_err(|error| fix_failure("FIX_CHILD_PREPARE_FAILED", error.public_message()))?;
    Ok(())
}

async fn prepare_fix_proposal(
    state: &AppState,
    request: AgentSessionRequest,
    graph: &WorkflowGraph,
) -> Result<(ValidatedAgentProposal, Option<String>), (&'static str, Option<String>)> {
    let base_version_id = request.base_version_id.clone();
    if !state.use_intent_contract {
        let proposed = state
            .agent
            .propose_graph_change(request)
            .await
            .map_err(|error| fix_failure("FIX_AGENT_FAILED", error.to_string()))?;
        if proposed.proposal.kind != ProposalKind::Fix {
            return Err(("FIX_PROPOSAL_INVALID", None));
        }
        let service = GraphService::new(NodeRegistry::builtin());
        let prepared = service
            .preview_proposal(
                graph,
                &proposed.proposal.base_version_id,
                ProposalDraft {
                    base_version_id: proposed.proposal.base_version_id.clone(),
                    kind: ProposalKind::Fix,
                    title: proposed.proposal.title.clone(),
                    summary: proposed.proposal.summary.clone(),
                    ops: proposed.proposal.ops.clone(),
                    message_id: None,
                },
            )
            .map_err(|error| fix_failure("FIX_PROPOSAL_INVALID", error.to_string()))?;
        return Ok((
            ValidatedAgentProposal {
                session_id: proposed.session_id,
                agent_logs: proposed.agent_logs,
                proposal: prepared,
            },
            None,
        ));
    }
    let validated = state
        .agent
        .propose_intent(request)
        .await
        .map_err(|error| fix_failure("FIX_AGENT_FAILED", error.to_string()))?;
    let service = GraphService::new(NodeRegistry::builtin());
    let availability = connector_availability(&state.provider_registry.catalog_snapshot());
    match compile(
        &validated.intent,
        graph,
        &service,
        helixflow_run::shared_catalog(),
        &availability,
    )
    .map_err(|error| fix_failure("FIX_PROPOSAL_INVALID", error.to_string()))?
    {
        CompileOutcome::Clarify(_) => Err(("FIX_AGENT_CLARIFICATION_REQUIRED", None)),
        CompileOutcome::Compiled(compiled) => {
            let semantics = serde_json::to_string(&compiled.target.collected_semantics())
                .map_err(|error| fix_failure("FIX_PROPOSAL_INVALID", error.to_string()))?;
            let prepared = service
                .preview_proposal(
                    graph,
                    &base_version_id,
                    ProposalDraft {
                        base_version_id: base_version_id.clone(),
                        kind: ProposalKind::Fix,
                        title: "Agent 自动修复".to_owned(),
                        summary: format!("catalog {}", compiled.catalog_revision),
                        ops: compiled.ops,
                        message_id: None,
                    },
                )
                .map_err(|error| fix_failure("FIX_PROPOSAL_INVALID", error.to_string()))?;
            Ok((
                ValidatedAgentProposal {
                    session_id: validated.session_id,
                    agent_logs: validated.agent_logs,
                    proposal: prepared,
                },
                Some(semantics),
            ))
        }
    }
}

async fn resume_fix_child(state: &AppState, attempt: &RunFixAttemptRecord) -> Result<(), ApiError> {
    let Some(target_version_id) = attempt.target_version_id.as_deref() else {
        return fail_fix_attempt(state, attempt, "FIX_RECOVERY_INTERRUPTED", None).await;
    };
    let version = state
        .store
        .version(target_version_id)
        .await
        .map_err(ApiError::store)?;
    let graph = read_version_graph(&state.data_dir, &version)
        .await
        .map_err(candidate_error)?;
    if let Err(error) = state.runner.prepare_run_fix_child(&attempt.id, graph).await {
        fail_fix_attempt(
            state,
            attempt,
            "FIX_CHILD_PREPARE_FAILED",
            safe_error_summary(&error.public_message()).as_deref(),
        )
        .await?;
    }
    Ok(())
}

async fn fail_fix_attempt(
    state: &AppState,
    attempt: &RunFixAttemptRecord,
    reason_code: &'static str,
    summary: Option<&str>,
) -> Result<(), ApiError> {
    state
        .store
        .fail_run_fix_attempt(&attempt.id, reason_code, summary)
        .await
        .map_err(ApiError::store)?;
    Ok(())
}

fn fix_failure(reason_code: &'static str, error: String) -> (&'static str, Option<String>) {
    (reason_code, safe_error_summary(&error))
}

async fn drain_run_fix_outbox(state: &AppState) -> Result<(), ApiError> {
    for outbox in state
        .store
        .pending_run_fix_outbox()
        .await
        .map_err(ApiError::store)?
    {
        let Some(event) = state
            .store
            .persist_run_fix_outbox_event(&outbox.id)
            .await
            .map_err(ApiError::store)?
        else {
            continue;
        };
        let run = state
            .store
            .run(&event.run_id)
            .await
            .map_err(ApiError::store)?;
        let data = serde_json::from_str(&event.data_json)
            .map_err(|error| ApiError::server_error(format!("invalid run fix event: {error}")))?;
        if state
            .events
            .publish(helixflow_run::RunEventEnvelope {
                workspace_id: run.workspace_id,
                run_id: event.run_id,
                seq: event.seq,
                server_time: event.created_at,
                ev: event.ev,
                data,
            })
            .is_ok()
        {
            state
                .store
                .mark_run_fix_outbox_broadcasted(&outbox.id)
                .await
                .map_err(ApiError::store)?;
        }
    }
    Ok(())
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
