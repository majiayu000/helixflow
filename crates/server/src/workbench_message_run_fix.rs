use std::collections::BTreeSet;

use helixflow_agent::{AgentSessionRequest, AgentSkill, TurnMode, ValidatedAgentProposal};
use helixflow_compiler::{CompileOutcome, compile_for_connector};
use helixflow_gateway::Provider;
use helixflow_graph::{GraphService, ProposalDraft, ProposalKind, WorkflowGraph};
use helixflow_registry::NodeRegistry;
use helixflow_store::{RunFixAttemptRecord, RunFixClaim, RunFixSnapshot};

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::version_file_consistency::read_version_graph;
use crate::workbench_message::{
    ensure_fix_scope, format_exact_debug_run_context, persist_agent_logs, safe_error_summary,
    safe_graph_projection,
};
use crate::workbench_message_intent_readiness::intent_compile_readiness;
use crate::workbench_message_proposals::persist_and_apply_run_fix_proposal;

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
        codex_thread_id: None,
        conversation_id: None,
        durable_turn_id: None,
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
    let selected_provider = state.selected_provider_for_workspace(&workspace);
    let (proposal, semantics_json) =
        prepare_fix_proposal(state, request, &graph, &selected_provider).await?;
    persist_agent_logs(
        state,
        &attempt.workspace_id,
        &proposal.session_id,
        &proposal.agent_logs,
        None,
        None,
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
    selected_provider: &str,
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
                runtime_identity: proposed.runtime_identity,
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
    let readiness = intent_compile_readiness(state, selected_provider, &validated.intent)
        .map_err(|code| fix_failure("FIX_PROVIDER_UNAVAILABLE", code))?;
    match compile_for_connector(
        &validated.intent,
        graph,
        &service,
        helixflow_run::shared_catalog(),
        &readiness.availability,
        readiness.connector_preference.as_deref(),
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
                    runtime_identity: validated.runtime_identity,
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
        .map_err(|error| ApiError::server_error(error.to_string()))?;
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
