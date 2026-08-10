use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_agent::{
    AgentError, AgentLogEntry, AgentRuntimeIdentity, AgentSessionRequest, TurnMode,
    ValidatedAgentProposal, ValidatedAgentReply,
};
use helixflow_graph::{PreparedProposal, WorkflowGraph};
use helixflow_run::EventBus;
use helixflow_store::{NewRun, NewRunStep, NewVersion, Store, VersionSource};

use crate::agent_turn_control::interrupt_workspace_agent_turn;
use crate::app_state::{AppState, WorkbenchAgent};
use crate::graph_files::graph_hash;
use crate::test_support::FailingWorkbenchAgent;
use crate::version_file_consistency::read_version_graph;
use crate::workbench_message::*;
use crate::workbench_message_proposals::{
    AutoApplyCommitHook, persist_and_apply_agent_proposal_with_hook,
};

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
            conversation_id: None,
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
    let conversations = state
        .store
        .workspace_conversations(&workspace_id)
        .await
        .expect("conversations");
    assert_eq!(
        conversations[0].codex_thread_id.as_deref(),
        Some("thr_fake")
    );
    let durable_turn = state
        .store
        .agent_turn(&response.turn_id)
        .await
        .expect("durable turn");
    assert_eq!(durable_turn.codex_turn_id.as_deref(), Some("turn_fake"));
}

#[tokio::test]
async fn failed_chat_persists_a_terminal_turn_and_visible_error_message() {
    let (mut state, workspace_id, version_id, _dir) = state_with_workspace().await;
    state.agent = Arc::new(FailingWorkbenchAgent);

    let response = post_workspace_message(
        Path(workspace_id.clone()),
        State(state.clone()),
        Json(WorkspaceMessageRequest {
            base_version_id: version_id,
            user_message: "你好".to_owned(),
            graph: sample_graph(),
            canvas_context: None,
            conversation_id: None,
        }),
    )
    .await
    .expect("terminal error response")
    .0;

    assert_eq!(response.turn_status, "error");
    assert_eq!(response.messages[0].kind, "agent_error");
    assert!(response.messages[0].text.contains("AGENT_RUNTIME_ERROR"));
    let turn = state
        .store
        .agent_turn(&response.turn_id)
        .await
        .expect("persisted turn");
    assert_eq!(turn.status, "error");
    assert_eq!(turn.reason_code.as_deref(), Some("AGENT_RUNTIME_ERROR"));
    assert!(turn.completed_at.is_some());
}

#[tokio::test]
async fn interrupting_active_chat_persists_an_interrupted_terminal_turn() {
    let (mut state, workspace_id, version_id, _dir) = state_with_workspace().await;
    let started = Arc::new(tokio::sync::Notify::new());
    state.agent = Arc::new(HangingWorkbenchAgent {
        started: started.clone(),
    });

    let request_state = state.clone();
    let request_workspace_id = workspace_id.clone();
    let request = tokio::spawn(async move {
        post_workspace_message(
            Path(request_workspace_id),
            State(request_state),
            Json(WorkspaceMessageRequest {
                base_version_id: version_id,
                user_message: "你好".to_owned(),
                graph: sample_graph(),
                canvas_context: None,
                conversation_id: None,
            }),
        )
        .await
    });
    started.notified().await;

    let interrupted =
        interrupt_workspace_agent_turn(Path(workspace_id.clone()), State(state.clone()))
            .await
            .expect("interrupt response")
            .0;
    assert_eq!(interrupted.status, "interrupt_requested");

    let response = request
        .await
        .expect("request task")
        .expect("terminal response")
        .0;
    assert_eq!(response.turn_status, "interrupted");
    assert_eq!(response.messages[0].kind, "agent_interrupted");
    let turn = state
        .store
        .agent_turn(&response.turn_id)
        .await
        .expect("turn");
    assert_eq!(turn.status, "interrupted");
    assert_eq!(turn.reason_code.as_deref(), Some("USER_INTERRUPTED"));
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
            conversation_id: None,
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
            conversation_id: None,
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
    let version_id = proposal
        .result_version_id
        .as_deref()
        .expect("applied version id");
    assert!(state.data_dir.join(&proposal.ops_path).exists());
    assert!(
        state
            .data_dir
            .join(proposal.preview_graph_path.as_ref().expect("preview path"))
            .exists()
    );
    assert!(!proposal.ops_path.contains("_fake"));
    assert!(
        !proposal
            .preview_graph_path
            .as_deref()
            .expect("preview")
            .contains("_fake")
    );
    let version = state
        .store
        .version(version_id)
        .await
        .expect("applied version");
    assert!(!version.graph_path.contains("_fake"));
    let bytes = tokio::fs::read(state.data_dir.join(&version.graph_path))
        .await
        .expect("applied graph bytes");
    assert_eq!(version.graph_hash, graph_hash(&bytes));
    assert_eq!(
        read_version_graph(&state.data_dir, &version)
            .await
            .expect("verified applied graph"),
        sample_graph()
    );
    assert_eq!(
        state
            .store
            .version_file_references(&version.graph_path)
            .await
            .expect("version refs"),
        vec![version.clone()]
    );
    assert_eq!(
        state
            .store
            .proposal_file_references(&proposal.ops_path)
            .await
            .expect("ops refs")
            .iter()
            .map(|reference| reference.proposal_id.as_str())
            .collect::<Vec<_>>(),
        vec![proposal_id.as_str()]
    );
    let preview_path = proposal
        .preview_graph_path
        .as_deref()
        .expect("preview path");
    assert_eq!(
        state
            .store
            .proposal_file_references(preview_path)
            .await
            .expect("preview refs")
            .iter()
            .map(|reference| reference.proposal_id.as_str())
            .collect::<Vec<_>>(),
        vec![proposal_id.as_str()]
    );
    let candidate_paths = BTreeSet::from([
        proposal.ops_path.as_str(),
        preview_path,
        version.graph_path.as_str(),
    ]);
    assert_eq!(candidate_paths.len(), 3);
    let entries = candidate_entry_names(&state, &workspace_id).await;
    assert_eq!(entries.len(), 3, "candidate entries: {entries:?}");
    assert_no_candidate_temps(&entries);

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
async fn post_message_auto_applies_store_conflict_cleans_three_candidates() {
    let (state, workspace_id, base_version_id, _dir) = state_with_workspace().await;
    let proposal = auto_apply_proposal(&base_version_id, "../../raw-session-id");
    let hook = AutoApplyCommitHook::new();
    let task_state = state.clone();
    let task_workspace_id = workspace_id.clone();
    let task_hook = hook.clone();
    let mut task = tokio::spawn(async move {
        persist_and_apply_agent_proposal_with_hook(
            &task_state,
            &task_workspace_id,
            &proposal,
            &task_hook,
        )
        .await
    });
    if tokio::time::timeout(
        std::time::Duration::from_secs(5),
        hook.wait_until_published(),
    )
    .await
    .is_err()
    {
        task.abort();
        panic!("auto-apply candidates did not reach published checkpoint");
    }
    let published_entries = candidate_entry_names(&state, &workspace_id).await;
    assert_eq!(published_entries.len(), 3, "entries: {published_entries:?}");
    assert_no_candidate_temps(&published_entries);
    assert!(
        published_entries
            .iter()
            .all(|name| !name.contains("raw-session-id"))
    );
    let published_paths: Vec<_> = published_entries
        .iter()
        .map(|name| format!("workspaces/{workspace_id}/graphs/{name}"))
        .collect();
    let base = state
        .store
        .version(&base_version_id)
        .await
        .expect("base version");
    let newer = state
        .store
        .create_version_after(
            NewVersion {
                workspace_id: &workspace_id,
                label: "Concurrent winner",
                source: VersionSource::Manual,
                graph_path: &base.graph_path,
                graph_hash: &base.graph_hash,
                parent_id: Some(&base_version_id),
                semantics_json: None,
            },
            &base_version_id,
        )
        .await
        .expect("advance current before auto Store call");
    let version_ids_before_release: Vec<_> = state
        .store
        .versions_for_workspace(&workspace_id)
        .await
        .expect("versions before release")
        .into_iter()
        .map(|version| version.id)
        .collect();
    if tokio::time::timeout(std::time::Duration::from_secs(5), hook.release_store())
        .await
        .is_err()
    {
        task.abort();
        panic!("auto-apply Store release barrier timed out");
    }
    let error = match tokio::time::timeout(std::time::Duration::from_secs(5), &mut task).await {
        Ok(result) => result
            .expect("auto-apply task")
            .expect_err("Store conflict"),
        Err(_) => {
            task.abort();
            panic!("auto-apply task did not finish after Store release");
        }
    };

    assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
    assert_eq!(
        state
            .store
            .workspace(&workspace_id)
            .await
            .expect("workspace")
            .cur_version_id
            .as_deref(),
        Some(newer.id.as_str())
    );
    assert_eq!(
        state
            .store
            .versions_for_workspace(&workspace_id)
            .await
            .expect("versions after conflict")
            .into_iter()
            .map(|version| version.id)
            .collect::<Vec<_>>(),
        version_ids_before_release
    );
    assert!(
        state
            .store
            .workspace_messages(&workspace_id)
            .await
            .expect("messages")
            .is_empty()
    );
    assert!(
        state
            .store
            .workspace_proposals(&workspace_id)
            .await
            .expect("proposals")
            .is_empty()
    );
    for path in published_paths {
        assert!(
            state
                .store
                .version_file_references(&path)
                .await
                .expect("version refs")
                .is_empty()
        );
        assert!(
            state
                .store
                .proposal_file_references(&path)
                .await
                .expect("proposal refs")
                .is_empty()
        );
    }
    assert!(
        candidate_entry_names(&state, &workspace_id)
            .await
            .is_empty()
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
            conversation_id: None,
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

fn auto_apply_proposal(base_version_id: &str, session_id: &str) -> ValidatedAgentProposal {
    ValidatedAgentProposal {
        session_id: session_id.to_owned(),
        runtime_identity: None,
        agent_logs: Vec::new(),
        proposal: PreparedProposal {
            base_version_id: base_version_id.to_owned(),
            kind: helixflow_graph::ProposalKind::Modify,
            title: "Test proposal".to_owned(),
            summary: "Test proposal ready.".to_owned(),
            ops: Vec::new(),
            diff_summary: Vec::new(),
            preview_graph: sample_graph(),
            state: helixflow_graph::ProposalState::Pending,
            message_id: None,
        },
    }
}

async fn candidate_entry_names(state: &AppState, workspace_id: &str) -> Vec<String> {
    let graph_dir = state
        .data_dir
        .join("workspaces")
        .join(workspace_id)
        .join("graphs");
    let mut entries = tokio::fs::read_dir(graph_dir).await.expect("candidate dir");
    let mut names = Vec::new();
    while let Some(entry) = entries.next_entry().await.expect("candidate entry") {
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    names
}

fn assert_no_candidate_temps(entries: &[String]) {
    assert!(
        entries
            .iter()
            .all(|name| !(name.starts_with(".hf-") && name.ends_with(".tmp"))),
        "candidate temp remains: {entries:?}"
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
    let graph_bytes = serde_json::to_vec_pretty(&sample_graph()).expect("graph json");
    let stored_graph_hash = graph_hash(&graph_bytes);
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Message graph",
            source: VersionSource::Manual,
            graph_path: "graphs/message.json",
            graph_hash: &stored_graph_hash,
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("create version");
    tokio::fs::create_dir_all(data_dir.join("graphs"))
        .await
        .expect("create graph dir");
    tokio::fs::write(data_dir.join("graphs/message.json"), graph_bytes)
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
        catalog_revision: None,
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
            runtime_identity: Some(AgentRuntimeIdentity {
                thread_id: "thr_fake".to_owned(),
                turn_id: "turn_fake".to_owned(),
            }),
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
            runtime_identity: Some(AgentRuntimeIdentity {
                thread_id: "thr_fake".to_owned(),
                turn_id: "turn_fake".to_owned(),
            }),
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

struct IntentWorkbenchAgent {
    intent: helixflow_compiler::IntentPlan,
}

struct HangingWorkbenchAgent {
    started: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl WorkbenchAgent for HangingWorkbenchAgent {
    async fn answer_chat(
        &self,
        _request: AgentSessionRequest,
    ) -> Result<ValidatedAgentReply, AgentError> {
        self.started.notify_waiters();
        std::future::pending().await
    }

    async fn propose_graph_change(
        &self,
        _request: AgentSessionRequest,
    ) -> Result<ValidatedAgentProposal, AgentError> {
        Err(AgentError::Runtime(
            "graph change is not under test".to_owned(),
        ))
    }
}

#[async_trait]
impl WorkbenchAgent for IntentWorkbenchAgent {
    async fn answer_chat(
        &self,
        _request: AgentSessionRequest,
    ) -> Result<ValidatedAgentReply, AgentError> {
        Err(AgentError::Runtime("chat is not under test".to_owned()))
    }

    async fn propose_graph_change(
        &self,
        _request: AgentSessionRequest,
    ) -> Result<ValidatedAgentProposal, AgentError> {
        Err(AgentError::Runtime(
            "legacy proposal contract must not be used when the intent flag is on".to_owned(),
        ))
    }

    async fn propose_intent(
        &self,
        request: AgentSessionRequest,
    ) -> Result<helixflow_agent::ValidatedAgentIntent, AgentError> {
        assert!(request.use_intent_contract, "flag must reach the request");
        Ok(helixflow_agent::ValidatedAgentIntent {
            session_id: format!("{}_intent", request.workspace_id),
            runtime_identity: None,
            agent_logs: Vec::new(),
            intent: self.intent.clone(),
        })
    }
}

fn intent_plan(json: serde_json::Value) -> helixflow_compiler::IntentPlan {
    serde_json::from_value(json).expect("intent parses")
}

async fn intent_state(
    intent: helixflow_compiler::IntentPlan,
) -> (AppState, String, String, tempfile::TempDir) {
    let (mut state, workspace_id, version_id, dir) = state_with_workspace().await;
    state.agent = std::sync::Arc::new(IntentWorkbenchAgent { intent });
    state.use_intent_contract = true;
    // A healthy atlas connector so catalog resolution succeeds in tests.
    let atlas = helixflow_gateway::RuntimeProvider::Atlas(helixflow_gateway::AtlasProvider::new(
        helixflow_gateway::ApiProviderConfig::atlas(
            "test-key".to_owned(),
            "https://atlas.invalid/v1".to_owned(),
        ),
    ));
    state.provider_registry = helixflow_gateway::ProviderRegistry::new("atlas", vec![atlas]);
    (state, workspace_id, version_id, dir)
}

#[tokio::test]
async fn intent_turn_compiles_and_persists_semantics() {
    let intent = intent_plan(serde_json::json!({
        "intentVersion": "1",
        "topology": "linear",
        "stages": [{
            "stageId": "s1",
            "capabilityId": "text_to_image",
            "requestedModel": "Nano Banana",
            "inputFrom": [],
            "params": { "prompt": "a product image" }
        }],
        "outputStageIds": ["s1"]
    }));
    let (state, workspace_id, version_id, _dir) = intent_state(intent).await;

    let response = post_workspace_message(
        Path(workspace_id.clone()),
        State(state.clone()),
        Json(WorkspaceMessageRequest {
            base_version_id: version_id.clone(),
            user_message: "用 Nano Banana 创建一个生成图片的 workflow".to_owned(),
            graph: sample_graph(),
            canvas_context: None,
            conversation_id: None,
        }),
    )
    .await
    .expect("intent response")
    .0;

    assert_eq!(response.turn_mode, TurnMode::CreateWorkflow);
    assert_eq!(response.messages[0].kind, "proposal_applied");

    // The applied version carries the frozen semantic layer.
    let versions = state
        .store
        .versions_for_workspace(&workspace_id)
        .await
        .expect("versions");
    let applied = versions
        .iter()
        .find(|version| version.id != version_id)
        .expect("applied version");
    let semantics_json = applied
        .semantics_json
        .as_deref()
        .expect("semantics persisted");
    let semantics: std::collections::BTreeMap<
        String,
        helixflow_graph::semantics::NodeSemanticsEntry,
    > = serde_json::from_str(semantics_json).expect("semantics parse");
    let entry = semantics.get("s1").expect("s1 semantics");
    assert_eq!(entry.capability_id, "text_to_image");
    assert!(matches!(
        entry.implementation,
        helixflow_registry::catalog::ImplementationSelection::Pinned { ref requested_model_id, .. }
            if requested_model_id == "google/nano-banana-2"
    ));
}

#[tokio::test]
async fn intent_turn_missing_input_produces_clarify_message() {
    let intent = intent_plan(serde_json::json!({
        "intentVersion": "1",
        "topology": "linear",
        "stages": [{
            "stageId": "s1",
            "capabilityId": "text_to_image",
            "inputFrom": [],
            "params": {}
        }],
        "outputStageIds": ["s1"]
    }));
    let (state, workspace_id, version_id, _dir) = intent_state(intent).await;

    let response = post_workspace_message(
        Path(workspace_id.clone()),
        State(state.clone()),
        Json(WorkspaceMessageRequest {
            base_version_id: version_id.clone(),
            user_message: "创建一个生成图片的 workflow".to_owned(),
            graph: sample_graph(),
            canvas_context: None,
            conversation_id: None,
        }),
    )
    .await
    .expect("clarify response")
    .0;

    assert_eq!(response.messages[0].kind, "clarify");
    assert!(response.messages[0].text.contains("REQUIRED_INPUT_MISSING"));
    assert!(response.messages[0].text.contains("s1.prompt"));
    // A clarification never masquerades as an applied proposal.
    let proposals = state
        .store
        .workspace_proposals(&workspace_id)
        .await
        .expect("proposals");
    assert!(proposals.is_empty());
}

#[tokio::test]
async fn intent_turn_missing_model_binding_explains_catalog_gap() {
    let intent = intent_plan(serde_json::json!({
        "intentVersion": "1",
        "topology": "linear",
        "stages": [
            {
                "stageId": "s1",
                "capabilityId": "text_to_image",
                "requestedModel": "Nano Banana",
                "inputFrom": [],
                "params": { "prompt": "一张产品图" }
            },
            {
                "stageId": "s2",
                "capabilityId": "image_to_video",
                "requestedModel": "Seedance 2",
                "inputFrom": [{ "stageId": "s1", "output": "image" }],
                "params": {}
            }
        ],
        "outputStageIds": ["s2"]
    }));
    let (state, workspace_id, version_id, _dir) = intent_state(intent).await;

    let response = post_workspace_message(
        Path(workspace_id.clone()),
        State(state.clone()),
        Json(WorkspaceMessageRequest {
            base_version_id: version_id,
            user_message: "用 Nano Banana 生图，再用 Seedance 做成视频 workflow".to_owned(),
            graph: sample_graph(),
            canvas_context: None,
            conversation_id: None,
        }),
    )
    .await
    .expect("missing binding is a clarification response")
    .0;

    assert_eq!(response.messages[0].kind, "clarify");
    assert!(response.messages[0].text.contains("BINDING_NOT_FOUND"));
    assert!(response.messages[0].text.contains("Seedance 1.5 Pro"));
    assert!(response.messages[0].text.contains("Image To Video"));
    assert!(response.messages[0].text.contains("没有自动替换模型或能力"));
    assert!(response.messages[0].text.contains("Text To Video"));
    assert!(
        state
            .store
            .workspace_proposals(&workspace_id)
            .await
            .expect("proposals")
            .is_empty()
    );
}
