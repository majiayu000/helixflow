//! IntentPlan turn handling (GH130 T6): agent intent → deterministic compile
//! → validated proposal auto-apply with a persisted semantic layer, or a
//! structured clarification message. Rollback: HELIXFLOW_AGENT_INTENT_CONTRACT=0
//! restores the low-level proposal contract.

use helixflow_agent::{AgentSessionRequest, TurnMode, ValidatedAgentProposal};
use helixflow_compiler::{ClarifyFirst, CompileOutcome, CompiledProposal, compile};
use helixflow_graph::{GraphService, ProposalDraft, ProposalKind, WorkflowGraph};
use helixflow_registry::NodeRegistry;
use helixflow_store::NewMessage;
use serde_json::json;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::catalog_routes::connector_availability;
use crate::workbench_message::{ChatMessagePayload, WorkspaceMessageResponse, persist_agent_logs};
use crate::workbench_message_proposals::persist_and_apply_agent_proposal;

/// GH130 T6 grayscale switch: on unless explicitly disabled. `0` / `false` /
/// `off` restore the legacy proposal contract for rollback drills.
pub(crate) fn intent_contract_enabled() -> bool {
    match std::env::var("HELIXFLOW_AGENT_INTENT_CONTRACT") {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off"
        ),
        Err(_) => true,
    }
}

pub(crate) async fn handle_intent_turn(
    state: &AppState,
    workspace_id: &str,
    base_version_id: &str,
    base_graph: &WorkflowGraph,
    mode: TurnMode,
    request: AgentSessionRequest,
) -> Result<WorkspaceMessageResponse, ApiError> {
    let validated = state
        .agent
        .propose_intent(request)
        .await
        .map_err(ApiError::agent)?;
    persist_agent_logs(
        state,
        workspace_id,
        &validated.session_id,
        &validated.agent_logs,
    )
    .await?;

    let catalog = helixflow_run::shared_catalog();
    let availability = connector_availability(&state.provider_registry.catalog_snapshot());
    let service = GraphService::new(NodeRegistry::builtin());

    match compile(
        &validated.intent,
        base_graph,
        &service,
        catalog,
        &availability,
    ) {
        Ok(CompileOutcome::Compiled(compiled)) => {
            let semantics_json = serde_json::to_string(&compiled.semantics)
                .map_err(|err| ApiError::server_error(err.to_string()))?;
            let draft = ProposalDraft {
                base_version_id: base_version_id.to_owned(),
                kind: proposal_kind_for_mode(mode),
                title: proposal_title(&compiled),
                summary: proposal_summary(&compiled),
                ops: compiled.ops.clone(),
                message_id: None,
            };
            let prepared = service
                .preview_proposal(base_graph, base_version_id, draft)
                .map_err(|err| {
                    ApiError::conflict_with_details(
                        format!("compiled proposal failed preview: {err}"),
                        json!({ "code": "COMPILED_PROPOSAL_INVALID" }),
                    )
                })?;
            let proposal = ValidatedAgentProposal {
                session_id: validated.session_id,
                agent_logs: Vec::new(),
                proposal: prepared,
            };
            let message = persist_and_apply_agent_proposal(
                state,
                workspace_id,
                &proposal,
                Some(&semantics_json),
            )
            .await?;
            Ok(WorkspaceMessageResponse {
                turn_mode: mode,
                messages: vec![ChatMessagePayload::from_record(message)],
                proposal: None,
                run: None,
                pending_confirmation: None,
            })
        }
        Ok(CompileOutcome::Clarify(clarify)) => {
            let text = clarify_message_text(&clarify);
            let message = state
                .store
                .create_message(NewMessage {
                    workspace_id,
                    role: "agent",
                    kind: "clarify",
                    text: Some(&text),
                    ref_id: None,
                    attachment_ids_json: None,
                })
                .await
                .map_err(ApiError::store)?;
            Ok(WorkspaceMessageResponse {
                turn_mode: mode,
                messages: vec![ChatMessagePayload::from_record(message)],
                proposal: None,
                run: None,
                pending_confirmation: None,
            })
        }
        Err(err) => Err(ApiError::conflict_with_details(
            err.to_string(),
            json!({ "code": err.code(), "catalogRevision": catalog.catalog_revision }),
        )),
    }
}

fn proposal_kind_for_mode(mode: TurnMode) -> ProposalKind {
    match mode {
        TurnMode::CreateWorkflow => ProposalKind::Create,
        TurnMode::DebugWorkflow => ProposalKind::Fix,
        _ => ProposalKind::Modify,
    }
}

fn proposal_title(compiled: &CompiledProposal) -> String {
    format!("AI 编排（{} 个阶段）", compiled.resolved_stages.len())
}

fn proposal_summary(compiled: &CompiledProposal) -> String {
    let stages: Vec<String> = compiled
        .resolved_stages
        .iter()
        .map(|stage| {
            format!(
                "{}: {} → {}",
                stage.stage_id, stage.capability_id, stage.resolved_model_id
            )
        })
        .collect();
    format!(
        "catalog {} · {}",
        compiled
            .catalog_revision
            .chars()
            .take(14)
            .collect::<String>(),
        stages.join("；")
    )
}

/// Human-readable clarification. The stable machine fields lead each line so
/// the frontend clarify card can render them without a side channel, and the
/// message is never presented as a successful proposal.
fn clarify_message_text(clarify: &ClarifyFirst) -> String {
    let mut lines = vec![
        format!("需要澄清 [{}]", clarify.reason_code),
        clarify.next_action.clone(),
    ];
    if !clarify.missing_fields.is_empty() {
        lines.push(format!("缺失：{}", clarify.missing_fields.join("、")));
    }
    lines.join("\n")
}
