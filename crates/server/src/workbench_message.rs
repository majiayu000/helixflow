use std::path::PathBuf;

use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_agent::{AgentLogEntry, AgentSessionRequest, TurnMode, classify_turn_mode};
use helixflow_graph::{ProposalKind, WorkflowGraph};
use helixflow_run::AgentRunRequest;
use helixflow_store::{MessageRecord, NewMessage, NewProposal, RunRecord, RunStepRecord};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::write_json_file;
use crate::workbench_payload::{
    PendingConfirmationPayload, ProposalPayload, RunPayload, pending_confirmation_from_pending,
    proposal_payload_from_prepared, run_payload_from_pending,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceMessageRequest {
    base_version_id: String,
    user_message: String,
    graph: WorkflowGraph,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceMessageResponse {
    turn_mode: TurnMode,
    messages: Vec<ChatMessagePayload>,
    proposal: Option<ProposalPayload>,
    run: Option<RunPayload>,
    pending_confirmation: Option<PendingConfirmationPayload>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatMessagePayload {
    id: String,
    role: String,
    kind: String,
    text: String,
    time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn_mode: Option<TurnMode>,
}

pub(crate) async fn post_workspace_message(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
    Json(input): Json<WorkspaceMessageRequest>,
) -> Result<Json<WorkspaceMessageResponse>, ApiError> {
    let turn_mode = classify_turn_mode(&input.user_message, &input.graph)
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    let turn_metadata = turn_metadata_json(turn_mode);
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
    let run_context = debug_run_context(&state, &workspace_id, turn_mode).await?;
    let request = AgentSessionRequest {
        workspace_id: workspace_id.clone(),
        base_version_id: input.base_version_id,
        user_message: input.user_message,
        graph: input.graph,
        run_context,
        sessions_dir: state.agent_sessions_dir.clone(),
        mode: turn_mode,
        skill: turn_mode.agent_skill(),
    };

    match turn_mode {
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
                turn_mode,
                messages: vec![ChatMessagePayload::from_record(message)],
                proposal: None,
                run: None,
                pending_confirmation: None,
            }))
        }
        TurnMode::CreateWorkflow | TurnMode::ModifyWorkflow | TurnMode::DebugWorkflow => {
            let mut proposal = state
                .agent
                .propose_graph_change(request)
                .await
                .map_err(ApiError::agent)?;
            let (ops_path, preview_graph_path) =
                proposal_storage_paths(&workspace_id, &proposal.session_id);
            write_json_file(
                &state.data_dir,
                &ops_path,
                &proposal.proposal.ops,
                "write proposal ops",
            )
            .await?;
            write_json_file(
                &state.data_dir,
                &preview_graph_path,
                &proposal.proposal.preview_graph,
                "write proposal preview graph",
            )
            .await?;
            let ops_path_string = ops_path.to_string_lossy().into_owned();
            let preview_graph_path_string = preview_graph_path.to_string_lossy().into_owned();
            let proposal_record = state
                .store
                .create_proposal(NewProposal {
                    workspace_id: &workspace_id,
                    base_version_id: &proposal.proposal.base_version_id,
                    kind: proposal_kind_as_str(proposal.proposal.kind),
                    title: &proposal.proposal.title,
                    summary: &proposal.proposal.summary,
                    ops_path: &ops_path_string,
                    preview_graph_path: Some(&preview_graph_path_string),
                    message_id: None,
                })
                .await
                .map_err(ApiError::store)?;
            let message = state
                .store
                .create_message(NewMessage {
                    workspace_id: &workspace_id,
                    role: "agent",
                    kind: "proposal_pending",
                    text: Some(&proposal.proposal.summary),
                    ref_id: Some(&proposal_record.id),
                    attachment_ids_json: None,
                })
                .await
                .map_err(ApiError::store)?;
            proposal.proposal.message_id = Some(message.id.clone());
            let proposal_record = state
                .store
                .attach_proposal_message(&proposal_record.id, &message.id)
                .await
                .map_err(ApiError::store)?;
            persist_agent_logs(
                &state,
                &workspace_id,
                &proposal.session_id,
                &proposal.agent_logs,
            )
            .await?;
            Ok(Json(WorkspaceMessageResponse {
                turn_mode,
                messages: vec![ChatMessagePayload::from_record(message)],
                proposal: Some(proposal_payload_from_prepared(
                    proposal_record.id,
                    &proposal.proposal,
                )),
                run: None,
                pending_confirmation: None,
            }))
        }
        TurnMode::RunRequest => {
            let pending = state
                .runner
                .request_agent_run(AgentRunRequest {
                    workspace_id: request.workspace_id.clone(),
                    version_id: request.base_version_id.clone(),
                    group_id: None,
                    label: run_label(&request.user_message),
                    graph: request.graph,
                })
                .await
                .map_err(ApiError::run)?;
            let message_text = format!("Run {} is waiting for confirmation.", pending.run.id);
            let message = state
                .store
                .create_message(NewMessage {
                    workspace_id: &workspace_id,
                    role: "agent",
                    kind: "run_requested",
                    text: Some(&message_text),
                    ref_id: Some(&pending.run.id),
                    attachment_ids_json: None,
                })
                .await
                .map_err(ApiError::store)?;
            Ok(Json(WorkspaceMessageResponse {
                turn_mode,
                messages: vec![ChatMessagePayload::from_record(message)],
                proposal: None,
                run: Some(run_payload_from_pending(&pending)),
                pending_confirmation: Some(pending_confirmation_from_pending(&pending)),
            }))
        }
    }
}

async fn persist_agent_logs(
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
    fn from_record(record: MessageRecord) -> Self {
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

fn turn_metadata_json(turn_mode: TurnMode) -> String {
    json!({ "turnMode": turn_mode }).to_string()
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
        .latest_workspace_run(workspace_id)
        .await
        .map_err(ApiError::store)?
    else {
        return Ok(Some(
            "No recent run is available for this workspace.".to_owned(),
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
        "Latest run: id={}, status={}, label={}",
        run.id, run.status, run.label
    )];
    if let Some(error_json) = run.error_json.as_deref().map(truncate_debug_text) {
        lines.push(format!("Run error: {error_json}"));
    }
    for step in steps.iter().filter(|step| step.state == "failed") {
        lines.push(format!(
            "Failed step: node_id={}, node_type={}, provider={}",
            step.node_id,
            step.node_type,
            step.provider.as_deref().unwrap_or("none")
        ));
        if let Some(error_json) = step.error_json.as_deref().map(truncate_debug_text) {
            lines.push(format!("Step error: {error_json}"));
        }
    }
    lines.join("\n")
}

fn truncate_debug_text(value: &str) -> String {
    const MAX_DEBUG_TEXT: usize = 800;
    value.chars().take(MAX_DEBUG_TEXT).collect()
}

fn run_label(user_message: &str) -> String {
    let trimmed = user_message.trim();
    if trimmed.is_empty() {
        return "Agent requested run".to_owned();
    }
    trimmed.chars().take(80).collect()
}

fn proposal_storage_paths(workspace_id: &str, session_id: &str) -> (PathBuf, PathBuf) {
    let dir = PathBuf::from("workspaces")
        .join(workspace_id)
        .join("proposals")
        .join(session_id);
    (dir.join("ops.json"), dir.join("preview.json"))
}

fn proposal_kind_as_str(kind: ProposalKind) -> &'static str {
    match kind {
        ProposalKind::Create => "create",
        ProposalKind::Modify => "modify",
        ProposalKind::Fix => "fix",
        ProposalKind::Sweep => "sweep",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::extract::{Path, State};
    use helixflow_agent::{
        AgentError, AgentSessionRequest, ValidatedAgentProposal, ValidatedAgentReply,
    };
    use helixflow_graph::{PreparedProposal, WorkflowGraph};
    use helixflow_run::EventBus;
    use helixflow_store::{NewRun, NewRunStep, NewVersion, Store, VersionSource};

    use super::*;
    use crate::app_state::{AppState, WorkbenchAgent};

    #[tokio::test]
    async fn post_message_routes_chat_to_agent_runtime() {
        let (state, workspace_id, version_id, _dir) = state_with_workspace().await;

        let response = post_workspace_message(
            Path(workspace_id.clone()),
            State(state.clone()),
            Json(WorkspaceMessageRequest {
                base_version_id: version_id,
                user_message: "你好".to_owned(),
                graph: sample_graph(),
            }),
        )
        .await
        .expect("response")
        .0;

        assert_eq!(response.turn_mode, TurnMode::Chat);
        assert_eq!(response.proposal, None);
        assert_eq!(response.run, None);
        assert_eq!(response.pending_confirmation, None);
        assert_eq!(response.messages.len(), 1);
        assert_eq!(response.messages[0].kind, "chat");
        assert_eq!(response.messages[0].text, "我是 Helixflow agent。");

        let persisted = state
            .store
            .workspace_messages(&workspace_id)
            .await
            .expect("messages");
        assert_eq!(persisted.len(), 3);
        assert_eq!(persisted[0].role, "user");
        assert_eq!(
            persisted[0].attachment_ids_json.as_deref(),
            Some(r#"{"turnMode":"chat"}"#)
        );
        assert_eq!(persisted[1].role, "agent");
        assert_eq!(persisted[1].kind, "chat");
        assert_eq!(persisted[2].kind, "agent_log:status");
        let expected_session_id = format!("{workspace_id}_fake");
        assert_eq!(
            persisted[2].ref_id.as_deref(),
            Some(expected_session_id.as_str())
        );
    }

    #[tokio::test]
    async fn post_message_creates_pending_run_request() {
        let (state, workspace_id, version_id, _dir) = state_with_workspace().await;

        let response = post_workspace_message(
            Path(workspace_id.clone()),
            State(state.clone()),
            Json(WorkspaceMessageRequest {
                base_version_id: version_id,
                user_message: "运行当前 workflow".to_owned(),
                graph: sample_graph(),
            }),
        )
        .await
        .expect("run request response")
        .0;

        assert_eq!(response.turn_mode, TurnMode::RunRequest);
        assert_eq!(response.messages[0].kind, "run_requested");
        assert_eq!(
            response.run.as_ref().expect("run").status,
            "waiting_confirmation"
        );
        assert_eq!(
            response
                .pending_confirmation
                .as_ref()
                .expect("confirmation")
                .id,
            response.run.as_ref().expect("run").id
        );

        let persisted = state
            .store
            .workspace_messages(&workspace_id)
            .await
            .expect("messages");
        assert_eq!(persisted.len(), 2);
        assert_eq!(persisted[0].text.as_deref(), Some("运行当前 workflow"));
        assert_eq!(
            persisted[0].attachment_ids_json.as_deref(),
            Some(r#"{"turnMode":"run_request"}"#)
        );
        assert_eq!(persisted[1].kind, "run_requested");
        let run = state
            .store
            .run(&response.run.expect("run").id)
            .await
            .expect("persisted run");
        assert_eq!(run.status, "waiting_confirmation");
    }

    #[tokio::test]
    async fn post_message_persists_pending_proposal_record() {
        let (state, workspace_id, version_id, _dir) = state_with_workspace().await;

        let response = post_workspace_message(
            Path(workspace_id.clone()),
            State(state.clone()),
            Json(WorkspaceMessageRequest {
                base_version_id: version_id,
                user_message: "创建一个 workflow".to_owned(),
                graph: sample_graph(),
            }),
        )
        .await
        .expect("proposal response")
        .0;

        assert_eq!(response.turn_mode, TurnMode::CreateWorkflow);
        let proposal_id = response.proposal.as_ref().expect("proposal").id.clone();
        let proposal = state.store.proposal(&proposal_id).await.expect("proposal");
        assert_eq!(proposal.state, "pending");
        assert_eq!(proposal.kind, "modify");
        assert!(state.data_dir.join(&proposal.ops_path).exists());
        assert!(
            state
                .data_dir
                .join(proposal.preview_graph_path.as_ref().expect("preview path"))
                .exists()
        );

        let persisted = state
            .store
            .workspace_messages(&workspace_id)
            .await
            .expect("messages");
        assert_eq!(persisted[1].kind, "proposal_pending");
        assert_eq!(persisted[1].ref_id.as_deref(), Some(proposal_id.as_str()));
        assert_eq!(persisted[2].kind, "agent_log:status");
    }

    #[tokio::test]
    async fn post_message_routes_debug_with_latest_run_context() {
        let (state, workspace_id, version_id, _dir) = state_with_workspace().await;
        let run = state
            .store
            .create_run(NewRun {
                workspace_id: &workspace_id,
                version_id: &version_id,
                group_id: None,
                label: "Failed render",
                trigger: "manual",
                plan_json: None,
                estimate_json: None,
                status: "running",
            })
            .await
            .expect("create run");
        let step = state
            .store
            .create_run_step(NewRunStep {
                run_id: &run.id,
                node_id: "video",
                node_type: "video.mock.text_to_video",
                provider: Some("mock"),
                state: "running",
            })
            .await
            .expect("create run step");
        let error_json = r#"{"error":"provider rejected duration"}"#;
        state
            .store
            .update_run_step_state(&step.id, "failed", Some(1.0), None, Some(error_json))
            .await
            .expect("fail step");
        state
            .store
            .update_run_status(&run.id, "failed", Some(error_json))
            .await
            .expect("fail run");

        let response = post_workspace_message(
            Path(workspace_id.clone()),
            State(state.clone()),
            Json(WorkspaceMessageRequest {
                base_version_id: version_id,
                user_message: "为什么失败了，帮我修复".to_owned(),
                graph: sample_graph(),
            }),
        )
        .await
        .expect("debug response")
        .0;

        assert_eq!(response.turn_mode, TurnMode::DebugWorkflow);
        assert!(response.proposal.is_some());
        let persisted = state
            .store
            .workspace_messages(&workspace_id)
            .await
            .expect("messages");
        assert_eq!(
            persisted[0].attachment_ids_json.as_deref(),
            Some(r#"{"turnMode":"debug_workflow"}"#)
        );
    }

    async fn state_with_workspace() -> (AppState, String, String, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store
            .create_workspace("Message workspace")
            .await
            .expect("create workspace");
        let version = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Message graph",
                source: VersionSource::Manual,
                graph_path: "graphs/message.json",
                graph_hash: "sha256:message",
                parent_id: None,
            })
            .await
            .expect("create version");
        let state = AppState::with_store_agent(
            EventBus::new(16),
            store,
            data_dir.clone(),
            Arc::new(FakeWorkbenchAgent),
            data_dir.join("sessions"),
        );
        (state, workspace.id, version.id, dir)
    }

    fn sample_graph() -> WorkflowGraph {
        WorkflowGraph {
            schema_version: 1,
            nodes: BTreeMap::new(),
            edges: Vec::new(),
        }
    }

    struct FakeWorkbenchAgent;

    #[async_trait]
    impl WorkbenchAgent for FakeWorkbenchAgent {
        async fn answer_chat(
            &self,
            request: AgentSessionRequest,
        ) -> Result<ValidatedAgentReply, AgentError> {
            Ok(ValidatedAgentReply {
                session_id: format!("{}_fake", request.workspace_id),
                agent_logs: vec![AgentLogEntry {
                    kind: "agent_log:status".to_owned(),
                    text: "fake runtime status".to_owned(),
                }],
                message: "我是 Helixflow agent。".to_owned(),
            })
        }

        async fn propose_graph_change(
            &self,
            request: AgentSessionRequest,
        ) -> Result<ValidatedAgentProposal, AgentError> {
            if request.mode == TurnMode::DebugWorkflow {
                let context = request
                    .run_context
                    .as_deref()
                    .ok_or_else(|| AgentError::Runtime("missing debug run context".to_owned()))?;
                if !context.contains("Latest run")
                    || !context.contains("provider rejected duration")
                    || !context.contains("Failed step")
                {
                    return Err(AgentError::Runtime(format!(
                        "incomplete debug run context: {context}"
                    )));
                }
            }
            Ok(ValidatedAgentProposal {
                session_id: format!("{}_fake", request.workspace_id),
                agent_logs: vec![AgentLogEntry {
                    kind: "agent_log:status".to_owned(),
                    text: "fake proposal status".to_owned(),
                }],
                proposal: PreparedProposal {
                    base_version_id: request.base_version_id,
                    kind: helixflow_graph::ProposalKind::Modify,
                    title: "Test proposal".to_owned(),
                    summary: "Test proposal ready.".to_owned(),
                    ops: Vec::new(),
                    diff_summary: Vec::new(),
                    preview_graph: request.graph,
                    state: helixflow_graph::ProposalState::Pending,
                    message_id: None,
                },
            })
        }
    }
}
