use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_agent::{
    AgentError, AgentLogEntry, AgentSessionRequest, TurnMode, ValidatedAgentProposal,
    ValidatedAgentReply,
};
use helixflow_graph::{PreparedProposal, WorkflowGraph};
use helixflow_run::EventBus;
use helixflow_store::{NewRun, NewRunStep, NewVersion, Store, VersionSource};

use crate::app_state::{AppState, WorkbenchAgent};
use crate::workbench_message::*;

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
            canvas_context: None,
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
async fn post_message_auto_starts_free_run_request() {
    let (state, workspace_id, version_id, _dir) = state_with_workspace().await;
    let mut events = state.events.subscribe();

    let response = post_workspace_message(
        Path(workspace_id.clone()),
        State(state.clone()),
        Json(WorkspaceMessageRequest {
            base_version_id: version_id,
            user_message: "运行当前 workflow".to_owned(),
            graph: sample_graph(),
            canvas_context: None,
        }),
    )
    .await
    .expect("run request response")
    .0;

    assert_eq!(response.turn_mode, TurnMode::RunRequest);
    assert_eq!(response.messages[0].kind, "run_requested");
    assert_eq!(response.run.as_ref().expect("run").status, "running");
    assert_eq!(response.pending_confirmation, None);
    assert!(
        response.messages[0]
            .text
            .contains("automatic cost approval")
    );
    let run_id = response.run.as_ref().expect("run").id.clone();
    let requested = std::iter::from_fn(|| events.try_recv().ok())
        .find(|event| event.run_id == run_id && event.ev == "run.requested")
        .expect("run.requested event");
    assert_eq!(requested.data["requires_confirmation"], false);

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
    assert!(matches!(run.status.as_str(), "running" | "succeeded"));
}

#[tokio::test]
async fn post_message_auto_applies_proposal_record() {
    let (state, workspace_id, version_id, _dir) = state_with_workspace().await;

    let response = post_workspace_message(
        Path(workspace_id.clone()),
        State(state.clone()),
        Json(WorkspaceMessageRequest {
            base_version_id: version_id,
            user_message: "创建一个 workflow".to_owned(),
            graph: sample_graph(),
            canvas_context: None,
        }),
    )
    .await
    .expect("proposal response")
    .0;

    assert_eq!(response.turn_mode, TurnMode::CreateWorkflow);
    assert_eq!(response.proposal, None);
    assert_eq!(response.run, None);
    assert_eq!(response.pending_confirmation, None);
    assert_eq!(response.messages[0].kind, "proposal_applied");
    let proposals = state
        .store
        .workspace_proposals(&workspace_id)
        .await
        .expect("proposals");
    assert_eq!(proposals.len(), 1);
    let proposal_id = proposals[0].id.clone();
    let proposal = state.store.proposal(&proposal_id).await.expect("proposal");
    assert_eq!(proposal.state, "applied");
    assert_eq!(proposal.kind, "modify");
    assert!(proposal.result_version_id.is_some());
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
    assert_eq!(persisted[1].kind, "agent_log:status");
    assert_eq!(persisted[2].kind, "proposal_applied");
    assert_eq!(persisted[2].ref_id.as_deref(), Some(proposal_id.as_str()));
    assert_eq!(
        persisted[2]
            .attachment_ids_json
            .as_deref()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
            .and_then(|value| value["versionId"].as_str().map(str::to_owned)),
        proposal.result_version_id
    );
    assert!(
        state
            .store
            .latest_pending_proposal(&workspace_id)
            .await
            .expect("pending proposal")
            .is_none()
    );
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
            node_type: "video.text_to_video",
            provider: Some("mock"),
            state: "running",
        })
        .await
        .expect("create run step");
    let error_json = r#"{"error":"provider rejected duration OPENAI_API_KEY=sk-secret-value Authorization: Bearer abc123 token leaked-token password hunter2","trace":"raw stack line"}"#;
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
    state
        .store
        .create_run(NewRun {
            workspace_id: &workspace_id,
            version_id: &version_id,
            group_id: None,
            label: "Succeeded later",
            trigger: "manual",
            plan_json: None,
            estimate_json: None,
            status: "succeeded",
        })
        .await
        .expect("create later run");

    let response = post_workspace_message(
        Path(workspace_id.clone()),
        State(state.clone()),
        Json(WorkspaceMessageRequest {
            base_version_id: version_id.clone(),
            user_message: "为什么失败了，帮我修复".to_owned(),
            graph: sample_graph(),
            canvas_context: None,
        }),
    )
    .await
    .expect("debug response")
    .0;

    assert_eq!(response.turn_mode, TurnMode::DebugWorkflow);
    assert_eq!(response.proposal, None);
    assert_eq!(response.messages[0].kind, "proposal_applied");
    assert_ne!(
        state
            .store
            .workspace(&workspace_id)
            .await
            .expect("workspace")
            .cur_version_id
            .as_deref(),
        Some(version_id.as_str())
    );
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

pub(super) async fn state_with_workspace() -> (AppState, String, String, tempfile::TempDir) {
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
    tokio::fs::create_dir_all(data_dir.join("graphs"))
        .await
        .expect("create graph dir");
    tokio::fs::write(
        data_dir.join("graphs/message.json"),
        serde_json::to_vec_pretty(&sample_graph()).expect("graph json"),
    )
    .await
    .expect("write graph");
    let state = AppState::with_store_agent(
        EventBus::new(16),
        store,
        data_dir.clone(),
        Arc::new(FakeWorkbenchAgent),
        data_dir.join("sessions"),
    );
    (state, workspace.id, version.id, dir)
}

pub(super) fn sample_graph() -> WorkflowGraph {
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
            if !context.contains("Latest failed run")
                || !context.contains("provider rejected duration")
                || !context.contains("Failed step")
            {
                return Err(AgentError::Runtime(format!(
                    "incomplete debug run context: {context}"
                )));
            }
            if context.contains("sk-secret")
                || context.contains("OPENAI_API_KEY")
                || context.contains("abc123")
                || context.contains("leaked-token")
                || context.contains("hunter2")
                || context.contains("raw stack")
                || context.contains(r#"{"error""#)
            {
                return Err(AgentError::Runtime(format!(
                    "unsafe debug run context: {context}"
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
