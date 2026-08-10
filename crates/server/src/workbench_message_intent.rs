//! IntentPlan turn handling (GH130 T6): agent intent → deterministic compile
//! → validated proposal auto-apply with a persisted semantic layer, or a
//! structured clarification message. Rollback: HELIXFLOW_AGENT_INTENT_CONTRACT=0
//! restores the low-level proposal contract.

use helixflow_agent::{AgentSessionRequest, TurnMode, ValidatedAgentProposal};
use helixflow_compiler::{ClarifyFirst, CompileOutcome, CompiledProposal, compile};
use helixflow_graph::{GraphService, ProposalDraft, ProposalKind, WorkflowGraph};
use helixflow_registry::NodeRegistry;
use helixflow_store::{AgentContractOutcome, NewMessage};

use crate::agent_contract_observation::{
    AgentContractTurn, agent_error_code, finalize_agent_contract_error,
};
use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::catalog_routes::connector_availability;
use crate::workbench_message::{
    ChatMessagePayload, WorkspaceMessageResponse, persist_agent_logs, terminal_error_response,
};
use crate::workbench_message_proposals::persist_and_apply_agent_proposal_observed;

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
    turn: &AgentContractTurn,
    conversation_id: &str,
    durable_turn_id: &str,
) -> Result<WorkspaceMessageResponse, ApiError> {
    let validated = match state.agent.propose_intent(request).await {
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
    if let Err(_error) = persist_agent_logs(
        state,
        workspace_id,
        &validated.session_id,
        &validated.agent_logs,
        Some(conversation_id),
        Some(durable_turn_id),
    )
    .await
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
            let semantics_json = match serde_json::to_string(&compiled.target.collected_semantics())
            {
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
                title: proposal_title(&compiled),
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
            let proposal = ValidatedAgentProposal {
                session_id: validated.session_id.clone(),
                agent_logs: Vec::new(),
                proposal: prepared,
            };
            let applied = persist_and_apply_agent_proposal_observed(
                state,
                workspace_id,
                &proposal,
                Some(&semantics_json),
                turn.completion(
                    workspace_id,
                    AgentContractOutcome::Success,
                    "INTENT_COMPILED",
                    Some(&validated.session_id),
                ),
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
            let message = state
                .store
                .assign_message_context(&message.id, conversation_id, Some(durable_turn_id))
                .await
                .map_err(ApiError::store)?;
            state
                .store
                .finalize_agent_turn(
                    durable_turn_id,
                    "succeeded",
                    None,
                    Some(&validated.session_id),
                )
                .await
                .map_err(ApiError::store)?;
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
        Ok(CompileOutcome::Clarify(clarify)) => {
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

#[cfg(test)]
mod run_agent_fix_tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use helixflow_agent::{
        AgentError, AgentSessionRequest, TurnMode, ValidatedAgentIntent, ValidatedAgentProposal,
        ValidatedAgentReply,
    };
    use helixflow_graph::{
        GraphEdge, GraphNode, GraphService, ProposalDraft, ProposalKind, WorkflowGraph,
    };
    use helixflow_registry::NodeRegistry;
    use helixflow_run::{AgentFixPolicy, EventBus};
    use helixflow_store::{NewRun, NewRunStep, NewVersion, Store, VersionSource};
    use serde_json::json;

    use crate::app_state::{AppState, WorkbenchAgent};
    use crate::graph_files::graph_hash;
    use crate::workbench_message_run_fix::recover_run_agent_fixes_with_policy;

    struct FixAgent {
        calls: AtomicUsize,
        fail_first: bool,
    }

    #[async_trait]
    impl WorkbenchAgent for FixAgent {
        async fn answer_chat(
            &self,
            _request: AgentSessionRequest,
        ) -> Result<ValidatedAgentReply, AgentError> {
            Err(AgentError::Runtime("chat is not allowed".to_owned()))
        }

        async fn propose_graph_change(
            &self,
            request: AgentSessionRequest,
        ) -> Result<ValidatedAgentProposal, AgentError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            assert_eq!(request.mode, TurnMode::DebugWorkflow);
            assert_eq!(request.skill, helixflow_agent::AgentSkill::FixError);
            assert!(request.history.is_empty());
            let context = request.run_context.as_deref().expect("safe context");
            assert!(context.contains("<<<UNTRUSTED_RUN_DIAGNOSTICS>>>"));
            assert!(!context.contains("sk-secret"));
            if self.fail_first && call == 1 {
                return Err(AgentError::Runtime("transient agent failure".to_owned()));
            }
            let service = GraphService::new(NodeRegistry::builtin());
            let proposal = service
                .preview_proposal(
                    &request.graph,
                    &request.base_version_id,
                    ProposalDraft {
                        base_version_id: request.base_version_id.clone(),
                        kind: ProposalKind::Fix,
                        title: "Fix writer".to_owned(),
                        summary: "Change failed writer style".to_owned(),
                        ops: vec![helixflow_graph::ProposalOp::SetParam {
                            id: "writer".to_owned(),
                            key: "style".to_owned(),
                            prev: Some(json!("cinematic")),
                            value: json!("product"),
                        }],
                        message_id: None,
                    },
                )
                .expect("valid fix proposal");
            Ok(ValidatedAgentProposal {
                session_id: format!("fix-{call}"),
                agent_logs: Vec::new(),
                proposal,
            })
        }

        async fn propose_intent(
            &self,
            _request: AgentSessionRequest,
        ) -> Result<ValidatedAgentIntent, AgentError> {
            Err(AgentError::Runtime("intent is not enabled".to_owned()))
        }
    }

    #[tokio::test]
    async fn run_agent_fix_retries_agent_then_applies_fresh_child() {
        let _fix_env = FixEnv::enabled();
        let agent = Arc::new(FixAgent {
            calls: AtomicUsize::new(0),
            fail_first: true,
        });
        let (state, source_run_id, source_version_id, _dir) =
            failed_agent_state(agent.clone()).await;

        recover_run_agent_fixes_with_policy(
            &state,
            AgentFixPolicy {
                enabled: true,
                max_attempts: 2,
            },
        )
        .await
        .expect("recover fixes");

        assert_eq!(agent.calls.load(Ordering::SeqCst), 2);
        let workspace = state
            .store
            .workspace(
                &state
                    .store
                    .run(&source_run_id)
                    .await
                    .expect("source")
                    .workspace_id,
            )
            .await
            .expect("workspace");
        let target_version_id = workspace.cur_version_id.expect("target version");
        assert_ne!(target_version_id, source_version_id);
        let fix_applied = state
            .store
            .run_events(&source_run_id)
            .await
            .expect("source events")
            .into_iter()
            .find(|event| event.ev == "run.fix_applied")
            .expect("fix applied event");
        let child_id = serde_json::from_str::<serde_json::Value>(&fix_applied.data_json)
            .expect("fix event json")["child_run_id"]
            .as_str()
            .expect("child id")
            .to_owned();
        let child = state.store.run(&child_id).await.expect("child");
        assert_ne!(child.id, source_run_id);
        assert_eq!(child.version_id, target_version_id);
        let child_plan: helixflow_graph::ExecutionPlan =
            serde_json::from_str(child.plan_json.as_deref().expect("fresh plan"))
                .expect("plan json");
        assert_eq!(child_plan.version_id, target_version_id);
        let attempt = state
            .store
            .run_fix_attempt_for_child(&child.id)
            .await
            .expect("attempt lookup")
            .expect("attempt");
        assert_eq!(attempt.state, "child_ready");
        assert_eq!(attempt.attempt_index, 2);
        assert!(
            !state
                .store
                .cost_ledger_for_run(&child.id)
                .await
                .expect("child ledger")
                .is_empty()
        );
        assert_eq!(
            state
                .store
                .run(&source_run_id)
                .await
                .expect("source preserved")
                .status,
            "failed"
        );
    }

    struct FixEnv;

    impl FixEnv {
        fn enabled() -> Self {
            // SAFETY: this test restores both variables in Drop. The server
            // test binary has no production AppState worker using this flag.
            unsafe {
                std::env::set_var("HELIXFLOW_RUN_AGENT_FIX_ENABLED", "1");
                std::env::set_var("HELIXFLOW_RUN_MAX_FIX_ATTEMPTS", "2");
            }
            Self
        }
    }

    impl Drop for FixEnv {
        fn drop(&mut self) {
            // SAFETY: see FixEnv::enabled; removal restores the default-off
            // process state after the single end-to-end test.
            unsafe {
                std::env::remove_var("HELIXFLOW_RUN_AGENT_FIX_ENABLED");
                std::env::remove_var("HELIXFLOW_RUN_MAX_FIX_ATTEMPTS");
            }
        }
    }

    async fn failed_agent_state(
        agent: Arc<dyn WorkbenchAgent>,
    ) -> (AppState, String, String, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let store = Store::open(&format!(
            "sqlite://{}",
            data_dir.join("fix.sqlite").display()
        ))
        .await
        .expect("store");
        let workspace = store
            .create_workspace("Fix workspace")
            .await
            .expect("workspace");
        let graph = executable_graph();
        let bytes = serde_json::to_vec_pretty(&graph).expect("graph bytes");
        let hash = graph_hash(&bytes);
        tokio::fs::create_dir_all(data_dir.join("graphs"))
            .await
            .expect("graph dir");
        tokio::fs::write(data_dir.join("graphs/source.json"), bytes)
            .await
            .expect("graph file");
        let version = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Source",
                source: VersionSource::Manual,
                graph_path: "graphs/source.json",
                graph_hash: &hash,
                parent_id: None,
                semantics_json: None,
            })
            .await
            .expect("version");
        let plan = GraphService::new(NodeRegistry::builtin())
            .compile_plan(&graph, &version.id, "mock")
            .expect("source plan");
        let plan_json = serde_json::to_string(&plan).expect("plan json");
        let (run, _) = store
            .create_agent_run_with_repair_chain(NewRun {
                workspace_id: &workspace.id,
                version_id: &version.id,
                group_id: None,
                label: "Failed agent run",
                trigger: "agent",
                plan_json: Some(&plan_json),
                estimate_json: Some(
                    r#"{"amount":0.0,"currency":"USD","estimated":true,"unknown":false}"#,
                ),
                status: "running",
            })
            .await
            .expect("source run");
        let step = store
            .create_run_step(NewRunStep {
                run_id: &run.id,
                node_id: "writer",
                node_type: "llm.prompt_writer",
                provider: Some("mock"),
                state: "failed",
            })
            .await
            .expect("failed step");
        store
            .update_run_step_state(
                &step.id,
                "failed",
                Some(1.0),
                None,
                Some(r#"{"error":"provider failed Authorization sk-secret"}"#),
            )
            .await
            .expect("step error");
        store
            .request_run_terminalization(
                &run.id,
                "failed",
                Some(r#"{"error":"provider failed Authorization sk-secret"}"#),
            )
            .await
            .expect("terminal request");
        assert!(
            store
                .claim_run_terminalization(&run.id, "test", 60)
                .await
                .expect("claim terminal")
        );
        store
            .complete_run_terminalization(&run.id, "test")
            .await
            .expect("complete terminal");
        store
            .complete_failure_continuation(&run.id, true)
            .await
            .expect("retry exhausted");
        let state = AppState::with_store_agent(
            EventBus::new(32),
            store,
            data_dir.clone(),
            agent,
            data_dir.join("sessions"),
        );
        (state, run.id, version.id, dir)
    }

    fn executable_graph() -> WorkflowGraph {
        WorkflowGraph {
            schema_version: 1,
            catalog_revision: None,
            nodes: BTreeMap::from([
                (
                    "text".to_owned(),
                    GraphNode {
                        node_type: "input.text".to_owned(),
                        title: "Text".to_owned(),
                        params: json!({ "text": "launch teaser" }),
                        pos: [0.0, 0.0],
                        size: None,
                        semantics: None,
                    },
                ),
                (
                    "writer".to_owned(),
                    GraphNode {
                        node_type: "llm.prompt_writer".to_owned(),
                        title: "Writer".to_owned(),
                        params: json!({ "style": "cinematic" }),
                        pos: [1.0, 0.0],
                        size: None,
                        semantics: None,
                    },
                ),
            ]),
            edges: vec![GraphEdge {
                from: ["text".to_owned(), "text".to_owned()],
                to: ["writer".to_owned(), "text".to_owned()],
                edge_type: "text".to_owned(),
            }],
        }
    }
}
