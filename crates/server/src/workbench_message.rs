use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_agent::{AgentLogEntry, AgentSessionRequest, TurnMode, classify_turn_mode};
use helixflow_graph::WorkflowGraph;
use helixflow_store::{MessageRecord, NewMessage, RunRecord, RunStepRecord};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::sweep_support::handle_run_request;
use crate::workbench_message_canvas::{WorkspaceCanvasContext, prepare_agent_canvas_context};
use crate::workbench_message_graph::{VerifiedMessageGraph, verified_message_graph};
use crate::workbench_message_intent::handle_intent_turn;
use crate::workbench_message_metadata::turn_metadata_json;
use crate::workbench_message_proposals::persist_and_apply_agent_proposal;
use crate::workbench_payload::{PendingConfirmationPayload, ProposalPayload, RunPayload};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkspaceMessageRequest {
    pub(crate) base_version_id: String,
    pub(crate) user_message: String,
    pub(crate) graph: WorkflowGraph,
    #[serde(default)]
    pub(crate) canvas_context: Option<WorkspaceCanvasContext>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceMessageResponse {
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
}

pub(crate) async fn post_workspace_message(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
    Json(input): Json<WorkspaceMessageRequest>,
) -> Result<Json<WorkspaceMessageResponse>, ApiError> {
    let VerifiedMessageGraph { version, graph } =
        verified_message_graph(&state, &workspace_id, &input.base_version_id, &input.graph).await?;
    let classification = classify_turn_mode(&input.user_message, &graph)
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    let canvas_context = prepare_agent_canvas_context(
        &state,
        &workspace_id,
        classification.mode,
        &version.id,
        &graph,
        input.canvas_context.clone(),
    )
    .await?;
    // Prior turns give the agent cross-request memory (HF-013). Loaded
    // before persisting the new user message so it is not duplicated.
    let history = state
        .store
        .workspace_messages(&workspace_id)
        .await
        .map_err(ApiError::store)?
        .into_iter()
        .filter(|message| message.role == "user" || message.role == "agent")
        .filter_map(|message| {
            message
                .text
                .map(|text| helixflow_agent::AgentHistoryMessage {
                    role: message.role,
                    text,
                })
        })
        .collect();
    let turn_metadata = turn_metadata_json(classification);
    state
        .store
        .create_message(NewMessage {
            workspace_id: &workspace_id,
            role: "user",
            kind: "text",
            text: Some(&input.user_message),
            ref_id: None,
            attachment_ids_json: Some(&turn_metadata),
        })
        .await
        .map_err(ApiError::store)?;
    let workspace = state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    let provider_catalog = state.provider_catalog_for_workspace(&workspace);
    let run_context = debug_run_context(&state, &workspace_id, classification.mode).await?;
    let use_intent_contract = state.use_intent_contract;
    let base_version_id = version.id.clone();
    let base_graph = graph.clone();
    let request = AgentSessionRequest {
        workspace_id: workspace_id.clone(),
        base_version_id: version.id,
        user_message: input.user_message,
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

    match classification.mode {
        TurnMode::Chat => {
            let reply = state
                .agent
                .answer_chat(request)
                .await
                .map_err(ApiError::agent)?;
            let message = state
                .store
                .create_message(NewMessage {
                    workspace_id: &workspace_id,
                    role: "agent",
                    kind: "chat",
                    text: Some(&reply.message),
                    ref_id: Some(&reply.session_id),
                    attachment_ids_json: None,
                })
                .await
                .map_err(ApiError::store)?;
            persist_agent_logs(&state, &workspace_id, &reply.session_id, &reply.agent_logs).await?;
            Ok(Json(WorkspaceMessageResponse {
                turn_mode: classification.mode,
                messages: vec![ChatMessagePayload::from_record(message)],
                proposal: None,
                run: None,
                pending_confirmation: None,
            }))
        }
        TurnMode::CreateWorkflow | TurnMode::ModifyWorkflow | TurnMode::DebugWorkflow => {
            if use_intent_contract {
                let response = handle_intent_turn(
                    &state,
                    &workspace_id,
                    &base_version_id,
                    &base_graph,
                    classification.mode,
                    request,
                )
                .await?;
                return Ok(Json(response));
            }
            let proposal = state
                .agent
                .propose_graph_change(request)
                .await
                .map_err(ApiError::agent)?;
            persist_agent_logs(
                &state,
                &workspace_id,
                &proposal.session_id,
                &proposal.agent_logs,
            )
            .await?;
            let message =
                persist_and_apply_agent_proposal(&state, &workspace_id, &proposal, None).await?;
            Ok(Json(WorkspaceMessageResponse {
                turn_mode: classification.mode,
                messages: vec![ChatMessagePayload::from_record(message)],
                proposal: None,
                run: None,
                pending_confirmation: None,
            }))
        }
        TurnMode::RunRequest => {
            let run_request = handle_run_request(&state, request).await?;
            Ok(Json(WorkspaceMessageResponse {
                turn_mode: classification.mode,
                messages: vec![ChatMessagePayload::from_record(run_request.message)],
                proposal: None,
                run: Some(run_request.run),
                pending_confirmation: run_request.pending_confirmation,
            }))
        }
    }
}

pub(crate) async fn persist_agent_logs(
    state: &AppState,
    workspace_id: &str,
    session_id: &str,
    logs: &[AgentLogEntry],
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
            })
            .await
            .map_err(ApiError::store)?;
    }
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
        }
    }
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

async fn debug_run_context(
    state: &AppState,
    workspace_id: &str,
    turn_mode: TurnMode,
) -> Result<Option<String>, ApiError> {
    if turn_mode != TurnMode::DebugWorkflow {
        return Ok(None);
    }
    let Some(run) = state
        .store
        .latest_failed_workspace_run(workspace_id)
        .await
        .map_err(ApiError::store)?
    else {
        return Ok(Some(
            "No recent failed run is available for this workspace.".to_owned(),
        ));
    };
    let steps = state
        .store
        .run_steps(&run.id)
        .await
        .map_err(ApiError::store)?;
    Ok(Some(format_debug_run_context(&run, &steps)))
}

fn format_debug_run_context(run: &RunRecord, steps: &[RunStepRecord]) -> String {
    let mut lines = vec![format!(
        "Latest failed run: id={}, status={}, label={}",
        run.id, run.status, run.label
    )];
    if let Some(summary) = run.error_json.as_deref().and_then(safe_error_summary) {
        lines.push(format!("Run error summary: {summary}"));
    }
    for step in steps.iter().filter(|step| step.state == "failed") {
        lines.push(format!(
            "Failed step: node_id={}, node_type={}, provider={}",
            step.node_id,
            step.node_type,
            step.provider.as_deref().unwrap_or("none")
        ));
        if let Some(summary) = step.error_json.as_deref().and_then(safe_error_summary) {
            lines.push(format!("Step error summary: {summary}"));
        }
    }
    lines.join("\n")
}

fn safe_error_summary(value: &str) -> Option<String> {
    let summary = serde_json::from_str::<Value>(value)
        .ok()
        .and_then(|parsed| {
            ["error", "message", "reason"]
                .into_iter()
                .find_map(|key| parsed.get(key).and_then(Value::as_str))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| value.to_owned());
    let redacted = redact_debug_text(first_line(summary.trim()));
    let summary = truncate_debug_text(redacted.trim());
    if summary.is_empty() {
        None
    } else {
        Some(summary)
    }
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or(value)
}

fn redact_debug_text(value: &str) -> String {
    let mut redacted = Vec::new();
    let mut redact_next = false;
    for token in value.split_whitespace() {
        if redact_next {
            redacted.push("[redacted]".to_owned());
            redact_next = is_sensitive_debug_label(token);
            continue;
        }
        if is_sensitive_debug_label(token) {
            redacted.push("[redacted]".to_owned());
            redact_next = true;
            continue;
        }
        redacted.push(redact_debug_token(token));
    }
    redacted.join(" ")
}

fn is_sensitive_debug_label(token: &str) -> bool {
    let normalized = token
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "token" | "secret" | "password" | "api_key" | "apikey" | "authorization" | "bearer"
    )
}

fn redact_debug_token(token: &str) -> String {
    token
        .split_whitespace()
        .map(redact_debug_token_segment)
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_debug_token_segment(token: &str) -> String {
    let lower = token.to_ascii_lowercase();
    if lower.contains("sk-")
        || lower.contains("ghp_")
        || lower.contains("gho_")
        || lower.contains("github_pat_")
        || lower.contains("hf_")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("token")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("authorization")
    {
        "[redacted]".to_owned()
    } else {
        token.to_owned()
    }
}

fn truncate_debug_text(value: &str) -> String {
    const MAX_DEBUG_TEXT: usize = 240;
    value.chars().take(MAX_DEBUG_TEXT).collect()
}

#[cfg(test)]
mod gh102_tests;
#[cfg(test)]
mod gh60_tests;

#[cfg(test)]
pub(super) mod tests {
    pub(super) use crate::workbench_message_tests::{sample_graph, state_with_workspace};
}
