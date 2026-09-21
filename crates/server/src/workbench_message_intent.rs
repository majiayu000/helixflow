//! Canvas-edit turn handling: agent operations → typed proposal auto-apply
//! with a persisted semantic layer, or a structured clarification message.

use helixflow_agent::{
    AgentSessionRequest, CanvasEditError, CanvasEditPlan, CompiledCanvasEdit, TurnMode,
    compile_canvas_edit_with,
};
use helixflow_compiler::ClarifyFirst;
use helixflow_graph::{GraphService, ProposalDraft, ProposalKind, WorkflowGraph};
use helixflow_registry::NodeRegistry;
use helixflow_store::{AgentContractOutcome, NewMessage};
use serde_json::json;

use crate::agent_contract_observation::{
    AgentContractTurn, agent_error_code, finalize_agent_contract_error,
};
use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::workbench_message::{
    ChatMessagePayload, WorkspaceMessageResponse, persist_agent_logs,
    persist_agent_runtime_identity, terminal_error_response,
};
use crate::workbench_message_intent_readiness::canvas_edit_readiness;
use crate::workbench_message_proposals::persist_and_apply_agent_proposal_observed;

pub(crate) async fn handle_intent_turn(
    state: &AppState,
    base_version_id: &str,
    base_graph: &WorkflowGraph,
    mode: TurnMode,
    request: AgentSessionRequest,
    turn: &AgentContractTurn,
) -> Result<WorkspaceMessageResponse, ApiError> {
    let workspace_id = request.workspace_id.clone();
    let conversation_id = request
        .conversation_id
        .clone()
        .ok_or_else(|| ApiError::server_error("canvas edit turn is missing a conversation id"))?;
    let durable_turn_id = request
        .durable_turn_id
        .clone()
        .ok_or_else(|| ApiError::server_error("canvas edit turn is missing a durable turn id"))?;
    let workspace_id = workspace_id.as_str();
    let conversation_id = conversation_id.as_str();
    let durable_turn_id = durable_turn_id.as_str();
    let validated = match state.agent.propose_canvas_edit(request).await {
        Ok(validated) => validated,
        Err(error) => {
            finalize_agent_contract_error(
                state,
                workspace_id,
                turn,
                agent_error_code(&error),
                None,
            )
            .await
            .map_err(ApiError::store)?;
            return terminal_error_response(
                state,
                workspace_id,
                conversation_id,
                durable_turn_id,
                mode,
                agent_error_code(&error),
                None,
            )
            .await;
        }
    };
    if persist_agent_runtime_identity(
        state,
        workspace_id,
        conversation_id,
        durable_turn_id,
        validated.runtime_identity.as_ref(),
    )
    .await
    .is_err()
    {
        finalize_agent_contract_error(
            state,
            workspace_id,
            turn,
            "CODEX_THREAD_PERSISTENCE_ERROR",
            Some(&validated.session_id),
        )
        .await
        .map_err(ApiError::store)?;
        return terminal_error_response(
            state,
            workspace_id,
            conversation_id,
            durable_turn_id,
            mode,
            "CODEX_THREAD_PERSISTENCE_ERROR",
            Some(&validated.session_id),
        )
        .await;
    }
    if persist_agent_logs(
        state,
        workspace_id,
        &validated.session_id,
        &validated.agent_logs,
        Some(conversation_id),
        Some(durable_turn_id),
    )
    .await
    .is_err()
    {
        finalize_agent_contract_error(
            state,
            workspace_id,
            turn,
            "AGENT_LOG_PERSISTENCE_ERROR",
            Some(&validated.session_id),
        )
        .await
        .map_err(ApiError::store)?;
        return terminal_error_response(
            state,
            workspace_id,
            conversation_id,
            durable_turn_id,
            mode,
            "AGENT_LOG_PERSISTENCE_ERROR",
            Some(&validated.session_id),
        )
        .await;
    }

    let workspace = state
        .store
        .workspace(workspace_id)
        .await
        .map_err(ApiError::store)?;
    let selected_provider = state.selected_provider_for_workspace(&workspace);
    let readiness = match canvas_edit_readiness(state, &selected_provider, &validated.edit) {
        Ok(readiness) => readiness,
        Err(reason) => {
            finalize_agent_contract_error(
                state,
                workspace_id,
                turn,
                &reason,
                Some(&validated.session_id),
            )
            .await
            .map_err(ApiError::store)?;
            return terminal_error_response(
                state,
                workspace_id,
                conversation_id,
                durable_turn_id,
                mode,
                &reason,
                Some(&validated.session_id),
            )
            .await;
        }
    };
    let service = GraphService::new(NodeRegistry::builtin());

    match compile_canvas_edit_with(
        &validated.edit,
        base_graph,
        &NodeRegistry::builtin(),
        &readiness.availability,
        readiness.connector_preference.as_deref(),
    ) {
        Ok(compiled) => {
            let semantics_json =
                match serde_json::to_string(&compiled.preview.collected_semantics()) {
                    Ok(semantics_json) => semantics_json,
                    Err(_error) => {
                        finalize_agent_contract_error(
                            state,
                            workspace_id,
                            turn,
                            "INTENT_SERIALIZATION_ERROR",
                            Some(&validated.session_id),
                        )
                        .await
                        .map_err(ApiError::store)?;
                        return terminal_error_response(
                            state,
                            workspace_id,
                            conversation_id,
                            durable_turn_id,
                            mode,
                            "INTENT_SERIALIZATION_ERROR",
                            Some(&validated.session_id),
                        )
                        .await;
                    }
                };
            let draft = ProposalDraft {
                base_version_id: base_version_id.to_owned(),
                kind: proposal_kind_for_mode(mode),
                title: proposal_title(&validated.edit, &compiled),
                summary: proposal_summary(&compiled),
                ops: compiled.ops.clone(),
                message_id: None,
            };
            let prepared = match service.preview_proposal(base_graph, base_version_id, draft) {
                Ok(prepared) => prepared,
                Err(_error) => {
                    finalize_agent_contract_error(
                        state,
                        workspace_id,
                        turn,
                        "COMPILED_PROPOSAL_INVALID",
                        Some(&validated.session_id),
                    )
                    .await
                    .map_err(ApiError::store)?;
                    return terminal_error_response(
                        state,
                        workspace_id,
                        conversation_id,
                        durable_turn_id,
                        mode,
                        "COMPILED_PROPOSAL_INVALID",
                        Some(&validated.session_id),
                    )
                    .await;
                }
            };
            let applied = persist_and_apply_agent_proposal_observed(
                state,
                workspace_id,
                &prepared,
                Some(&semantics_json),
                turn.completion(
                    workspace_id,
                    AgentContractOutcome::Success,
                    "INTENT_COMPILED",
                    Some(&validated.session_id),
                ),
                conversation_id,
                durable_turn_id,
            )
            .await;
            let message = match applied {
                Ok(message) => message,
                Err(_error) => {
                    finalize_agent_contract_error(
                        state,
                        workspace_id,
                        turn,
                        "PROPOSAL_APPLY_ERROR",
                        Some(&validated.session_id),
                    )
                    .await
                    .map_err(ApiError::store)?;
                    return terminal_error_response(
                        state,
                        workspace_id,
                        conversation_id,
                        durable_turn_id,
                        mode,
                        "PROPOSAL_APPLY_ERROR",
                        Some(&validated.session_id),
                    )
                    .await;
                }
            };
            Ok(WorkspaceMessageResponse {
                conversation_id: conversation_id.to_owned(),
                turn_id: durable_turn_id.to_owned(),
                turn_status: "succeeded".to_owned(),
                turn_mode: mode,
                messages: vec![ChatMessagePayload::from_record(message)],
                proposal: None,
                run: None,
                pending_confirmation: None,
            })
        }
        Err(error) if error.is_clarify() => {
            let clarify = clarify_from_edit_error(&error);
            let text = clarify_message_text(&clarify);
            let clarified = state
                .store
                .create_clarification_and_finalize_observation(
                    NewMessage {
                        workspace_id,
                        role: "agent",
                        kind: "clarify",
                        text: Some(&text),
                        ref_id: Some(&validated.session_id),
                        attachment_ids_json: None,
                        conversation_id: Some(conversation_id),
                        turn_id: Some(durable_turn_id),
                    },
                    turn.completion(
                        workspace_id,
                        AgentContractOutcome::Clarify,
                        &clarify.reason_code,
                        Some(&validated.session_id),
                    ),
                )
                .await
                .map_err(ApiError::store)?;
            state
                .store
                .finalize_agent_turn(
                    durable_turn_id,
                    "clarify",
                    Some(&clarify.reason_code),
                    Some(&validated.session_id),
                )
                .await
                .map_err(ApiError::store)?;
            Ok(WorkspaceMessageResponse {
                conversation_id: conversation_id.to_owned(),
                turn_id: durable_turn_id.to_owned(),
                turn_status: "clarify".to_owned(),
                turn_mode: mode,
                messages: vec![ChatMessagePayload::from_record(clarified.message)],
                proposal: None,
                run: None,
                pending_confirmation: None,
            })
        }
        Err(error) => {
            finalize_agent_contract_error(
                state,
                workspace_id,
                turn,
                error.code(),
                Some(&validated.session_id),
            )
            .await
            .map_err(ApiError::store)?;
            terminal_error_response(
                state,
                workspace_id,
                conversation_id,
                durable_turn_id,
                mode,
                error.code(),
                Some(&validated.session_id),
            )
            .await
        }
    }
}

fn proposal_kind_for_mode(mode: TurnMode) -> ProposalKind {
    match mode {
        TurnMode::CreateWorkflow => ProposalKind::Create,
        TurnMode::DebugWorkflow => ProposalKind::Fix,
        _ => ProposalKind::Modify,
    }
}

fn proposal_title(edit: &CanvasEditPlan, compiled: &CompiledCanvasEdit) -> String {
    edit.summary
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("画布编辑（{} 步）", compiled.ops.len()))
}

fn proposal_summary(compiled: &CompiledCanvasEdit) -> String {
    let nodes: Vec<String> = compiled
        .preview
        .nodes
        .iter()
        .map(|(id, node)| format!("{id}: {}", node.node_type))
        .collect();
    format!(
        "{} 个节点 · {}",
        compiled.preview.nodes.len(),
        nodes.join("；")
    )
}

fn clarify_from_edit_error(error: &CanvasEditError) -> ClarifyFirst {
    match error {
        CanvasEditError::MissingParam { node_id, param } => ClarifyFirst::new(
            "REQUIRED_INPUT_MISSING",
            vec![format!("{node_id}.{param}")],
            json!({}),
            format!("Provide `{param}` on `{node_id}` and resubmit canvas.edit."),
        ),
        CanvasEditError::Resolve {
            node_id,
            code,
            reason,
        } => ClarifyFirst::new(code, Vec::new(), json!({ "nodeId": node_id }), reason),
        other => ClarifyFirst::new(other.code(), Vec::new(), json!({}), other.to_string()),
    }
}

fn clarify_message_text(clarify: &ClarifyFirst) -> String {
    if clarify.reason_code == "BINDING_NOT_FOUND" {
        return binding_not_found_message_text(clarify);
    }
    let mut lines = vec![
        format!("需要澄清 [{}]", clarify.reason_code),
        clarify.next_action.clone(),
    ];
    if !clarify.missing_fields.is_empty() {
        lines.push(format!("缺失：{}", clarify.missing_fields.join("、")));
    }
    lines.join("\n")
}

fn binding_not_found_message_text(clarify: &ClarifyFirst) -> String {
    let context = &clarify.safe_context;
    let capability = context["capabilityName"]
        .as_str()
        .or_else(|| context["capabilityId"].as_str())
        .unwrap_or("该能力");
    let requested_model = context["requestedModelName"]
        .as_str()
        .or_else(|| context["requestedModelId"].as_str());

    let mut lines = vec![format!("需要澄清 [{}]", clarify.reason_code)];
    match requested_model {
        Some(model) => lines.push(format!(
            "{model} 当前没有已启用的「{capability}」绑定。工作流未创建，也没有自动替换模型或能力。"
        )),
        None => lines.push(format!(
            "当前 Catalog 没有已启用的「{capability}」绑定。工作流未创建。"
        )),
    }

    let available_models = display_names(context.get("availableModels"));
    if available_models.is_empty() {
        lines.push(format!(
            "可用替代：Catalog 中暂时没有支持「{capability}」的模型。"
        ));
    } else {
        lines.push(format!("可用模型：{}。", available_models.join("、")));
    }

    let supported_capabilities = display_names(context.get("requestedModelCapabilities"));
    if let Some(model) = requested_model
        && !supported_capabilities.is_empty()
    {
        lines.push(format!(
            "{model} 当前已启用的能力：{}。",
            supported_capabilities.join("、")
        ));
    }
    lines.push(format!(
        "下一步：{}。",
        clarify.next_action.trim_end_matches('。')
    ));
    lines.join("\n")
}

fn display_names(value: Option<&serde_json::Value>) -> Vec<&str> {
    value
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item["displayName"].as_str())
        .collect()
}
