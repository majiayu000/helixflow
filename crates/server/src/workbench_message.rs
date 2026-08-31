use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_agent::{
    AgentLogEntry, AgentRuntimeIdentity, AgentSessionRequest, TurnMode, explicit_turn_mode,
};
use helixflow_graph::WorkflowGraph;
use helixflow_store::{
    AgentContractOutcome, MessageRecord, NewAgentContractObservation, NewMessage,
};
use serde::{Deserialize, Serialize};

use crate::agent_contract_observation::{
    AgentContractTurn, agent_error_code, contract_mode, finalize_agent_contract_error,
};
use crate::agent_turn_control::DurableAgentTurnGuard;
use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::sweep_support::handle_run_request;
use crate::workbench_message_canvas::{WorkspaceCanvasContext, prepare_agent_canvas_context};
use crate::workbench_message_debug::debug_run_context;
pub(crate) use crate::workbench_message_debug::{
    ensure_fix_scope, format_exact_debug_run_context, safe_error_summary, safe_graph_projection,
};
use crate::workbench_message_graph::{VerifiedMessageGraph, verified_message_graph};
use crate::workbench_message_intent::handle_intent_turn;
use crate::workbench_message_metadata::turn_metadata_json;
use crate::workbench_message_proposals::persist_and_apply_agent_proposal_observed;
use crate::workbench_payload::{PendingConfirmationPayload, ProposalPayload, RunPayload};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkspaceMessageRequest {
    pub(crate) base_version_id: String,
    pub(crate) user_message: String,
    pub(crate) graph: WorkflowGraph,
    #[serde(default)]
    pub(crate) conversation_id: Option<String>,
    #[serde(default)]
    pub(crate) canvas_context: Option<WorkspaceCanvasContext>,
    /// A trusted UI surface can state its intent explicitly. The typed enum
    /// keeps unknown values fail-closed and avoids keyword routing drift.
    #[serde(default)]
    pub(crate) turn_mode: Option<TurnMode>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceMessageResponse {
    pub(crate) conversation_id: String,
    pub(crate) turn_id: String,
    pub(crate) turn_status: String,
    pub(crate) turn_mode: TurnMode,
    pub(crate) messages: Vec<ChatMessagePayload>,
    pub(crate) proposal: Option<ProposalPayload>,
    pub(crate) run: Option<RunPayload>,
    pub(crate) pending_confirmation: Option<PendingConfirmationPayload>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatMessagePayload {
    pub(crate) id: String,
    pub(crate) role: String,
    pub(crate) kind: String,
    pub(crate) text: String,
    pub(crate) time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) turn_mode: Option<TurnMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) turn_id: Option<String>,
}

pub(crate) async fn post_workspace_message(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
    Json(input): Json<WorkspaceMessageRequest>,
) -> Result<Json<WorkspaceMessageResponse>, ApiError> {
    let VerifiedMessageGraph {
        mut version,
        mut graph,
    } = verified_message_graph(&state, &workspace_id, &input.base_version_id, &input.graph).await?;
    if input.user_message.trim().is_empty() {
        return Err(ApiError::bad_request("empty agent turn is not allowed"));
    }
    let conversation = match input.conversation_id.as_deref() {
        Some(conversation_id) => state
            .store
            .conversation(&workspace_id, conversation_id)
            .await
            .map_err(ApiError::store)?,
        None => state
            .store
            .ensure_workspace_conversation(&workspace_id)
            .await
            .map_err(ApiError::store)?,
    };
    // Prior turns give the agent cross-request memory (HF-013). Loaded
    // before persisting the new user message so it is not duplicated.
    let history: Vec<_> = state
        .store
        .conversation_messages(&conversation.id)
        .await
        .map_err(ApiError::store)?
        .into_iter()
        .filter(|message| {
            message.role == "user"
                || (message.role == "agent" && !message.kind.starts_with("agent_log:"))
        })
        .filter_map(|message| {
            message
                .text
                .map(|text| helixflow_agent::AgentHistoryMessage {
                    role: message.role,
                    text,
                })
        })
        .collect();
    let mut workspace = state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    let mut provider_catalog = state.provider_catalog_for_workspace(&workspace);
    let durable_turn_id = format!("turn_{}", uuid::Uuid::now_v7().simple());
    let (active_turn_guard, mut interrupt_rx) = state
        .active_agent_turns
        .register(&workspace_id, &durable_turn_id)?;
    let classification = match input.turn_mode {
        Some(TurnMode::Route) => {
            return Err(ApiError::bad_request(
                "the internal routing mode cannot be selected explicitly",
            ));
        }
        Some(mode) => explicit_turn_mode(mode),
        None => {
            let selected_nodes = input
                .canvas_context
                .as_ref()
                .map(|context| context.selection.node_ids.len())
                .unwrap_or_default();
            let routing_request = AgentSessionRequest {
                workspace_id: workspace_id.clone(),
                base_version_id: version.id.clone(),
                user_message: input.user_message.clone(),
                codex_thread_id: None,
                conversation_id: None,
                durable_turn_id: None,
                history: history.clone(),
                graph: graph.clone(),
                provider_catalog: provider_catalog.clone(),
                run_context: Some(format!(
                    "Routing context: graph_nodes={}, graph_empty={}, selected_nodes={selected_nodes}",
                    graph.nodes.len(),
                    graph.nodes.is_empty(),
                )),
                sessions_dir: state.agent_sessions_dir.clone(),
                mode: TurnMode::Route,
                skill: TurnMode::Route.agent_skill(),
                canvas_context: None,
                use_intent_contract: false,
            };
            tokio::select! {
                result = state.agent.route_turn(routing_request) => result.map_err(|error| {
                    ApiError::service_unavailable(format!(
                        "agent semantic routing failed: {}",
                        agent_error_code(&error)
                    ))
                })?,
                _ = &mut interrupt_rx => {
                    return Err(ApiError::conflict("agent semantic routing was interrupted"));
                }
            }
        }
    };
    let canvas_context = prepare_agent_canvas_context(
        &state,
        &workspace_id,
        classification.mode,
        &version.id,
        &graph,
        input.canvas_context.clone(),
    )
    .await?;
    let run_context = debug_run_context(&state, &workspace_id, classification.mode).await?;
    if input.turn_mode.is_none() {
        let verified =
            verified_message_graph(&state, &workspace_id, &input.base_version_id, &input.graph)
                .await?;
        version = verified.version;
        graph = verified.graph;
        workspace = state
            .store
            .workspace(&workspace_id)
            .await
            .map_err(ApiError::store)?;
        provider_catalog = state.provider_catalog_for_workspace(&workspace);
    }
    let mut durable_turn_guard =
        DurableAgentTurnGuard::new(state.store.clone(), &durable_turn_id, None);
    let durable_turn = state
        .store
        .start_agent_turn_with_id(
            &durable_turn_id,
            &workspace_id,
            &conversation.id,
            &classification.mode.to_string(),
        )
        .await
        .map_err(ApiError::store)?;
    let turn_metadata = turn_metadata_json(classification);
    let user_message = NewMessage {
        workspace_id: &workspace_id,
        role: "user",
        kind: "text",
        text: Some(&input.user_message),
        ref_id: None,
        attachment_ids_json: Some(&turn_metadata),
        conversation_id: Some(&conversation.id),
        turn_id: Some(&durable_turn.id),
    };
    let (contract_turn, persisted_user_message_id) = if matches!(
        classification.mode,
        TurnMode::CreateWorkflow | TurnMode::ModifyWorkflow | TurnMode::DebugWorkflow
    ) {
        let mode = contract_mode(state.use_intent_contract);
        let observation_id = format!("aco_{}", uuid::Uuid::now_v7().simple());
        durable_turn_guard.attach_observation(&observation_id);
        let started = state
            .store
            .create_graph_edit_message_with_observation_id(
                NewAgentContractObservation {
                    user_message,
                    contract_mode: mode,
                    release_id: state.agent_contract_attribution.release_id.as_deref(),
                    build_revision: state.agent_contract_attribution.build_revision.as_deref(),
                },
                &observation_id,
            )
            .await
            .map_err(ApiError::store)?;
        (
            Some(AgentContractTurn::new(started.observation.id, mode)),
            started.message.id,
        )
    } else {
        let message = state
            .store
            .create_message(user_message)
            .await
            .map_err(ApiError::store)?;
        (None, message.id)
    };
    state
        .store
        .attach_turn_user_message(&durable_turn.id, &persisted_user_message_id)
        .await
        .map_err(ApiError::store)?;
    let use_intent_contract = state.use_intent_contract;
    let base_version_id = version.id.clone();
    let base_graph = graph.clone();
    let request = AgentSessionRequest {
        workspace_id: workspace_id.clone(),
        base_version_id: version.id,
        user_message: input.user_message,
        codex_thread_id: conversation.codex_thread_id.clone(),
        conversation_id: Some(conversation.id.clone()),
        durable_turn_id: Some(durable_turn.id.clone()),
        history,
        graph,
        provider_catalog,
        run_context,
        sessions_dir: state.agent_sessions_dir.clone(),
        mode: classification.mode,
        skill: classification.mode.agent_skill(),
        canvas_context,
        use_intent_contract,
    };

    let response = tokio::select! {
        response = async {
            match classification.mode {
        TurnMode::Route => Err(ApiError::server_error(
            "internal routing mode reached workbench dispatch",
        )),
        TurnMode::Chat => {
            let reply = match state.agent.answer_chat(request).await {
                Ok(reply) => reply,
                Err(error) => {
                    let reason = agent_error_code(&error);
                    return terminal_error_response(
                        &state,
                        &workspace_id,
                        &conversation.id,
                        &durable_turn.id,
                        classification.mode,
                        reason,
                        None,
                    )
                    .await
                    .map(Json);
                }
            };
            if persist_agent_runtime_identity(
                &state,
                &workspace_id,
                &conversation.id,
                &durable_turn.id,
                reply.runtime_identity.as_ref(),
            )
            .await
            .is_err()
            {
                return terminal_error_response(
                    &state,
                    &workspace_id,
                    &conversation.id,
                    &durable_turn.id,
                    classification.mode,
                    "CODEX_THREAD_PERSISTENCE_ERROR",
                    Some(&reply.session_id),
                )
                .await
                .map(Json);
            }
            let message = state
                .store
                .create_message(NewMessage {
                    workspace_id: &workspace_id,
                    role: "agent",
                    kind: "chat",
                    text: Some(&reply.message),
                    ref_id: Some(&reply.session_id),
                    attachment_ids_json: None,
                    conversation_id: Some(&conversation.id),
                    turn_id: Some(&durable_turn.id),
                })
                .await
                .map_err(ApiError::store)?;
            if persist_agent_logs(
                &state,
                &workspace_id,
                &reply.session_id,
                &reply.agent_logs,
                Some(&conversation.id),
                Some(&durable_turn.id),
            )
            .await
            .is_err()
            {
                return terminal_error_response(
                    &state,
                    &workspace_id,
                    &conversation.id,
                    &durable_turn.id,
                    classification.mode,
                    "AGENT_LOG_PERSISTENCE_ERROR",
                    Some(&reply.session_id),
                )
                .await
                .map(Json);
            }
            state
                .store
                .finalize_agent_turn(&durable_turn.id, "succeeded", None, Some(&reply.session_id))
                .await
                .map_err(ApiError::store)?;
            Ok(Json(WorkspaceMessageResponse {
                conversation_id: conversation.id.clone(),
                turn_id: durable_turn.id.clone(),
                turn_status: "succeeded".to_owned(),
                turn_mode: classification.mode,
                messages: vec![ChatMessagePayload::from_record(message)],
                proposal: None,
                run: None,
                pending_confirmation: None,
            }))
        }
        TurnMode::CreateWorkflow | TurnMode::ModifyWorkflow | TurnMode::DebugWorkflow => {
            let turn = contract_turn.as_ref().ok_or_else(|| {
                ApiError::server_error("graph-edit turn is missing a contract observation")
            })?;
            if use_intent_contract {
                let response = handle_intent_turn(
                    &state,
                    &base_version_id,
                    &base_graph,
                    classification.mode,
                    request,
                    turn,
                )
                .await?;
                return Ok(Json(response));
            }
            let proposal = match state.agent.propose_graph_change(request).await {
                Ok(proposal) => proposal,
                Err(error) => {
                    finalize_agent_contract_error(
                        &state,
                        &workspace_id,
                        turn,
                        agent_error_code(&error),
                        None,
                    )
                    .await
                    .map_err(ApiError::store)?;
                    return terminal_error_response(
                        &state,
                        &workspace_id,
                        &conversation.id,
                        &durable_turn.id,
                        classification.mode,
                        agent_error_code(&error),
                        None,
                    )
                    .await
                    .map(Json);
                }
            };
            if persist_agent_runtime_identity(
                &state,
                &workspace_id,
                &conversation.id,
                &durable_turn.id,
                proposal.runtime_identity.as_ref(),
            )
            .await
            .is_err()
            {
                finalize_agent_contract_error(
                    &state,
                    &workspace_id,
                    turn,
                    "CODEX_THREAD_PERSISTENCE_ERROR",
                    Some(&proposal.session_id),
                )
                .await
                .map_err(ApiError::store)?;
                return terminal_error_response(
                    &state,
                    &workspace_id,
                    &conversation.id,
                    &durable_turn.id,
                    classification.mode,
                    "CODEX_THREAD_PERSISTENCE_ERROR",
                    Some(&proposal.session_id),
                )
                .await
                .map(Json);
            }
            if let Err(_error) = persist_agent_logs(
                &state,
                &workspace_id,
                &proposal.session_id,
                &proposal.agent_logs,
                Some(&conversation.id),
                Some(&durable_turn.id),
            )
            .await
            {
                finalize_agent_contract_error(
                    &state,
                    &workspace_id,
                    turn,
                    "AGENT_LOG_PERSISTENCE_ERROR",
                    Some(&proposal.session_id),
                )
                .await
                .map_err(ApiError::store)?;
                return terminal_error_response(
                    &state,
                    &workspace_id,
                    &conversation.id,
                    &durable_turn.id,
                    classification.mode,
                    "AGENT_LOG_PERSISTENCE_ERROR",
                    Some(&proposal.session_id),
                )
                .await
                .map(Json);
            }
            let applied = persist_and_apply_agent_proposal_observed(
                &state,
                &workspace_id,
                &proposal,
                None,
                turn.completion(
                    &workspace_id,
                    AgentContractOutcome::Success,
                    "LEGACY_PROPOSAL_APPLIED",
                    Some(&proposal.session_id),
                ),
                &conversation.id,
                &durable_turn.id,
            )
            .await;
            let message = match applied {
                Ok(message) => message,
                Err(_error) => {
                    finalize_agent_contract_error(
                        &state,
                        &workspace_id,
                        turn,
                        "PROPOSAL_APPLY_ERROR",
                        Some(&proposal.session_id),
                    )
                    .await
                    .map_err(ApiError::store)?;
                    return terminal_error_response(
                        &state,
                        &workspace_id,
                        &conversation.id,
                        &durable_turn.id,
                        classification.mode,
                        "PROPOSAL_APPLY_ERROR",
                        Some(&proposal.session_id),
                    )
                    .await
                    .map(Json);
                }
            };
            Ok(Json(WorkspaceMessageResponse {
                conversation_id: conversation.id.clone(),
                turn_id: durable_turn.id.clone(),
                turn_status: "succeeded".to_owned(),
                turn_mode: classification.mode,
                messages: vec![ChatMessagePayload::from_record(message)],
                proposal: None,
                run: None,
                pending_confirmation: None,
            }))
        }
        TurnMode::RunRequest => {
            let run_request = match handle_run_request(&state, request).await {
                Ok(run_request) => run_request,
                Err(_error) => {
                    return terminal_error_response(
                        &state,
                        &workspace_id,
                        &conversation.id,
                        &durable_turn.id,
                        classification.mode,
                        "RUN_REQUEST_ERROR",
                        None,
                    )
                    .await
                    .map(Json);
                }
            };
            let message = state
                .store
                .assign_message_context(
                    &run_request.message.id,
                    &conversation.id,
                    Some(&durable_turn.id),
                )
                .await
                .map_err(ApiError::store)?;
            state
                .store
                .finalize_agent_turn(&durable_turn.id, "succeeded", None, None)
                .await
                .map_err(ApiError::store)?;
            Ok(Json(WorkspaceMessageResponse {
                conversation_id: conversation.id.clone(),
                turn_id: durable_turn.id.clone(),
                turn_status: "succeeded".to_owned(),
                turn_mode: classification.mode,
                messages: vec![ChatMessagePayload::from_record(message)],
                proposal: None,
                run: Some(run_request.run),
                pending_confirmation: run_request.pending_confirmation,
            }))
            }
        }
        } => response,
        _ = &mut interrupt_rx => {
            if let Some(turn) = contract_turn.as_ref() {
                finalize_agent_contract_error(
                    &state,
                    &workspace_id,
                    turn,
                    "USER_INTERRUPTED",
                    None,
                )
                .await
                .map_err(ApiError::store)?;
            }
            terminal_interrupted_response(
                &state,
                &workspace_id,
                &conversation.id,
                &durable_turn.id,
                classification.mode,
            )
            .await
            .map(Json)
        }
    };
    drop(active_turn_guard);
    if response.is_ok() {
        durable_turn_guard.disarm();
    }
    response
}

pub(crate) async fn persist_agent_logs(
    state: &AppState,
    workspace_id: &str,
    session_id: &str,
    logs: &[AgentLogEntry],
    conversation_id: Option<&str>,
    turn_id: Option<&str>,
) -> Result<(), ApiError> {
    for log in logs {
        state
            .store
            .create_message(NewMessage {
                workspace_id,
                role: "agent",
                kind: &log.kind,
                text: Some(&log.text),
                ref_id: Some(session_id),
                attachment_ids_json: None,
                conversation_id,
                turn_id,
            })
            .await
            .map_err(ApiError::store)?;
    }
    Ok(())
}

pub(crate) async fn persist_agent_runtime_identity(
    state: &AppState,
    workspace_id: &str,
    conversation_id: &str,
    durable_turn_id: &str,
    identity: Option<&AgentRuntimeIdentity>,
) -> Result<(), ApiError> {
    let Some(identity) = identity else {
        return Ok(());
    };
    state
        .store
        .bind_conversation_codex_thread(workspace_id, conversation_id, &identity.thread_id)
        .await
        .map_err(ApiError::store)?;
    state
        .store
        .attach_agent_turn_codex_identity(durable_turn_id, &identity.turn_id)
        .await
        .map_err(ApiError::store)?;
    Ok(())
}

impl ChatMessagePayload {
    pub(crate) fn from_record(record: MessageRecord) -> Self {
        let turn_mode = message_turn_mode(&record);
        Self {
            id: record.id,
            role: record.role,
            kind: record.kind,
            text: record.text.unwrap_or_default(),
            time: record.created_at,
            turn_mode,
            conversation_id: record.conversation_id,
            turn_id: record.turn_id,
        }
    }
}

pub(crate) async fn terminal_error_response(
    state: &AppState,
    workspace_id: &str,
    conversation_id: &str,
    turn_id: &str,
    turn_mode: TurnMode,
    reason_code: &str,
    execution_id: Option<&str>,
) -> Result<WorkspaceMessageResponse, ApiError> {
    let text = format!("Agent 执行失败 [{reason_code}]。本轮已结束，可以重试。");
    let message = state
        .store
        .create_message(NewMessage {
            workspace_id,
            role: "agent",
            kind: "agent_error",
            text: Some(&text),
            ref_id: execution_id,
            attachment_ids_json: None,
            conversation_id: Some(conversation_id),
            turn_id: Some(turn_id),
        })
        .await
        .map_err(ApiError::store)?;
    state
        .store
        .finalize_agent_turn(turn_id, "error", Some(reason_code), execution_id)
        .await
        .map_err(ApiError::store)?;
    Ok(WorkspaceMessageResponse {
        conversation_id: conversation_id.to_owned(),
        turn_id: turn_id.to_owned(),
        turn_status: "error".to_owned(),
        turn_mode,
        messages: vec![ChatMessagePayload::from_record(message)],
        proposal: None,
        run: None,
        pending_confirmation: None,
    })
}

pub(crate) async fn terminal_interrupted_response(
    state: &AppState,
    workspace_id: &str,
    conversation_id: &str,
    turn_id: &str,
    turn_mode: TurnMode,
) -> Result<WorkspaceMessageResponse, ApiError> {
    let message = state
        .store
        .create_message(NewMessage {
            workspace_id,
            role: "agent",
            kind: "agent_interrupted",
            text: Some("Agent 已停止。本轮已结束，可以重新发送。"),
            ref_id: None,
            attachment_ids_json: None,
            conversation_id: Some(conversation_id),
            turn_id: Some(turn_id),
        })
        .await
        .map_err(ApiError::store)?;
    state
        .store
        .finalize_agent_turn(turn_id, "interrupted", Some("USER_INTERRUPTED"), None)
        .await
        .map_err(ApiError::store)?;
    Ok(WorkspaceMessageResponse {
        conversation_id: conversation_id.to_owned(),
        turn_id: turn_id.to_owned(),
        turn_status: "interrupted".to_owned(),
        turn_mode,
        messages: vec![ChatMessagePayload::from_record(message)],
        proposal: None,
        run: None,
        pending_confirmation: None,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageMetadata {
    turn_mode: Option<TurnMode>,
}

fn message_turn_mode(record: &MessageRecord) -> Option<TurnMode> {
    record
        .attachment_ids_json
        .as_deref()
        .and_then(|value| serde_json::from_str::<MessageMetadata>(value).ok())
        .and_then(|metadata| metadata.turn_mode)
}

#[cfg(test)]
mod gh102_tests;
#[cfg(test)]
mod gh60_tests;

#[cfg(test)]
pub(super) mod tests {
    pub(super) use crate::workbench_message_tests::{sample_graph, state_with_workspace};
}
