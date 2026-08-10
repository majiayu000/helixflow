use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use axum::extract::{Path, State};
use helixflow_agent::{
    AgentError, AgentSessionRequest, ValidatedAgentProposal, ValidatedAgentReply,
};
use helixflow_graph::{GraphNode, WorkflowGraph};
use helixflow_run::EventBus;
use helixflow_store::{NewVersion, Store, VersionSource};
use serde_json::json;
use tokio::sync::Mutex;

use crate::app_state::{AppState, WorkbenchAgent};
use crate::graph_files::graph_hash;
use crate::workbench_message::{WorkspaceMessageRequest, post_workspace_message};

#[tokio::test]
async fn verified_read_valid_ingress_sends_server_graph_to_agent() {
    let fixture = message_fixture(StoredPayload::Valid).await;
    let response = post_workspace_message(
        Path(fixture.workspace_id.clone()),
        State(fixture.state.clone()),
        axum::Json(WorkspaceMessageRequest {
            base_version_id: fixture.base_version_id.clone(),
            user_message: "hello".to_owned(),
            graph: fixture.server_graph.clone(),
            canvas_context: None,
            conversation_id: None,
        }),
    )
    .await
    .expect("valid message response");

    assert_eq!(response.0.messages.len(), 1);
    assert_eq!(fixture.agent.invocations.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture.agent.graphs.lock().await.as_slice(),
        [fixture.server_graph]
    );
    assert_eq!(
        fixture
            .state
            .store
            .workspace_messages(&fixture.workspace_id)
            .await
            .expect("messages")
            .len(),
        2
    );
}

#[tokio::test]
async fn verified_read_stale_base_is_rejected_before_any_side_effect() {
    assert_rejected_ingress(IngressFault::StaleBase).await;
}

#[tokio::test]
async fn verified_read_tampered_client_graph_is_rejected_before_any_side_effect() {
    assert_rejected_ingress(IngressFault::TamperedClientGraph).await;
}

#[tokio::test]
async fn verified_read_missing_version_graph_is_rejected_before_any_side_effect() {
    assert_rejected_ingress(IngressFault::MissingFile).await;
}

#[tokio::test]
async fn verified_read_invalid_stored_hash_is_rejected_before_any_side_effect() {
    assert_rejected_ingress(IngressFault::InvalidStoredHash).await;
}

#[tokio::test]
async fn verified_read_mismatched_version_hash_is_rejected_before_any_side_effect() {
    assert_rejected_ingress(IngressFault::HashMismatch).await;
}

#[tokio::test]
async fn verified_read_invalid_version_json_is_rejected_before_any_side_effect() {
    assert_rejected_ingress(IngressFault::InvalidJson).await;
}

#[derive(Clone, Copy)]
enum IngressFault {
    StaleBase,
    TamperedClientGraph,
    MissingFile,
    InvalidStoredHash,
    HashMismatch,
    InvalidJson,
}

async fn assert_rejected_ingress(fault: IngressFault) {
    let payload = match fault {
        IngressFault::MissingFile => StoredPayload::Missing,
        IngressFault::InvalidStoredHash => StoredPayload::InvalidStoredHash,
        IngressFault::HashMismatch => StoredPayload::HashMismatch,
        IngressFault::InvalidJson => StoredPayload::InvalidJson,
        IngressFault::StaleBase | IngressFault::TamperedClientGraph => StoredPayload::Valid,
    };
    let fixture = message_fixture(payload).await;
    let request_base = fixture.base_version_id.clone();
    let mut client_graph = fixture.server_graph.clone();
    let expected_status = match fault {
        IngressFault::StaleBase => {
            create_new_current(&fixture, "newer", &fixture.server_graph).await;
            axum::http::StatusCode::CONFLICT
        }
        IngressFault::TamperedClientGraph => {
            client_graph
                .nodes
                .get_mut("input")
                .expect("input node")
                .title = "Tampered client title".to_owned();
            axum::http::StatusCode::CONFLICT
        }
        IngressFault::MissingFile
        | IngressFault::InvalidStoredHash
        | IngressFault::HashMismatch
        | IngressFault::InvalidJson => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
    };
    let versions_before = workspace_version_ids(&fixture).await;
    let current_before = fixture
        .state
        .store
        .workspace(&fixture.workspace_id)
        .await
        .expect("workspace before request")
        .cur_version_id;

    let error = post_workspace_message(
        Path(fixture.workspace_id.clone()),
        State(fixture.state.clone()),
        axum::Json(WorkspaceMessageRequest {
            base_version_id: request_base,
            user_message: "run and modify this workflow".to_owned(),
            graph: client_graph,
            canvas_context: None,
            conversation_id: None,
        }),
    )
    .await
    .expect_err("invalid ingress must fail closed");

    assert_eq!(error.status, expected_status);
    assert_eq!(fixture.agent.invocations.load(Ordering::SeqCst), 0);
    assert_eq!(workspace_version_ids(&fixture).await, versions_before);
    assert_eq!(
        fixture
            .state
            .store
            .workspace(&fixture.workspace_id)
            .await
            .expect("workspace after request")
            .cur_version_id,
        current_before
    );
    assert!(
        fixture
            .state
            .store
            .workspace_messages(&fixture.workspace_id)
            .await
            .expect("messages")
            .is_empty()
    );
    assert!(
        fixture
            .state
            .store
            .workspace_proposals(&fixture.workspace_id)
            .await
            .expect("proposals")
            .is_empty()
    );
    assert!(
        fixture
            .state
            .store
            .latest_workspace_run(&fixture.workspace_id)
            .await
            .expect("latest run")
            .is_none()
    );
    assert!(
        fixture
            .state
            .store
            .canvas_comment_state(&fixture.workspace_id)
            .await
            .expect("canvas comment state")
            .is_none()
    );
}

struct MessageFixture {
    state: AppState,
    workspace_id: String,
    base_version_id: String,
    server_graph: WorkflowGraph,
    agent: Arc<RecordingAgent>,
    _dir: tempfile::TempDir,
}

#[derive(Clone, Copy)]
enum StoredPayload {
    Valid,
    Missing,
    InvalidStoredHash,
    HashMismatch,
    InvalidJson,
}

async fn message_fixture(payload: StoredPayload) -> MessageFixture {
    let dir = tempfile::tempdir().expect("temp dir");
    let data_dir = dir.path().to_path_buf();
    let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    let workspace = store
        .create_workspace("Verified message workspace")
        .await
        .expect("create workspace");
    let server_graph = verified_graph();
    let graph_path = "graphs/message-verified.json".to_owned();
    let canonical_bytes = serde_json::to_vec(&server_graph).expect("graph json");
    let (file_bytes, stored_hash) = match payload {
        StoredPayload::Valid => (Some(canonical_bytes.clone()), graph_hash(&canonical_bytes)),
        StoredPayload::Missing => (None, graph_hash(&canonical_bytes)),
        StoredPayload::InvalidStoredHash => {
            (Some(canonical_bytes), "sha256:not-canonical".to_owned())
        }
        StoredPayload::HashMismatch => (
            Some(serde_json::to_vec(&tampered_graph()).expect("tampered graph json")),
            graph_hash(&canonical_bytes),
        ),
        StoredPayload::InvalidJson => {
            let bytes = b"not workflow json".to_vec();
            let hash = graph_hash(&bytes);
            (Some(bytes), hash)
        }
    };
    tokio::fs::create_dir_all(data_dir.join("graphs"))
        .await
        .expect("create graph dir");
    if let Some(file_bytes) = file_bytes {
        tokio::fs::write(data_dir.join(&graph_path), file_bytes)
            .await
            .expect("write graph");
    }
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Verified message graph",
            source: VersionSource::Manual,
            graph_path: &graph_path,
            graph_hash: &stored_hash,
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("create version");
    let agent = Arc::new(RecordingAgent::default());
    let state = AppState::with_store_agent(
        EventBus::new(16),
        store,
        data_dir.clone(),
        agent.clone(),
        data_dir.join("sessions"),
    );
    MessageFixture {
        state,
        workspace_id: workspace.id,
        base_version_id: version.id,
        server_graph,
        agent,
        _dir: dir,
    }
}

async fn create_new_current(fixture: &MessageFixture, suffix: &str, graph: &WorkflowGraph) {
    let path = format!("graphs/{suffix}.json");
    let bytes = serde_json::to_vec(graph).expect("new graph json");
    let hash = graph_hash(&bytes);
    tokio::fs::write(fixture.state.data_dir.join(&path), bytes)
        .await
        .expect("write new graph");
    fixture
        .state
        .store
        .create_version_after(
            NewVersion {
                workspace_id: &fixture.workspace_id,
                label: suffix,
                source: VersionSource::Manual,
                graph_path: &path,
                graph_hash: &hash,
                parent_id: Some(&fixture.base_version_id),
                semantics_json: None,
            },
            &fixture.base_version_id,
        )
        .await
        .expect("create new current");
}

async fn workspace_version_ids(fixture: &MessageFixture) -> Vec<String> {
    fixture
        .state
        .store
        .versions_for_workspace(&fixture.workspace_id)
        .await
        .expect("versions")
        .into_iter()
        .map(|version| version.id)
        .collect()
}

fn verified_graph() -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([(
            "input".to_owned(),
            GraphNode {
                node_type: "input.text".to_owned(),
                title: "Server-owned input".to_owned(),
                params: json!({ "text": "verified" }),
                pos: [0.0, 0.0],
                size: None,
                semantics: None,
            },
        )]),
        edges: Vec::new(),
        catalog_revision: None,
    }
}

fn tampered_graph() -> WorkflowGraph {
    let mut graph = verified_graph();
    graph.nodes.get_mut("input").expect("input node").params = json!({ "text": "tampered" });
    graph
}

#[derive(Default)]
struct RecordingAgent {
    invocations: AtomicUsize,
    graphs: Mutex<Vec<WorkflowGraph>>,
}

#[async_trait]
impl WorkbenchAgent for RecordingAgent {
    async fn answer_chat(
        &self,
        request: AgentSessionRequest,
    ) -> Result<ValidatedAgentReply, AgentError> {
        self.invocations.fetch_add(1, Ordering::SeqCst);
        self.graphs.lock().await.push(request.graph);
        Ok(ValidatedAgentReply {
            session_id: "verified-message-session".to_owned(),
            agent_logs: Vec::new(),
            message: "verified reply".to_owned(),
        })
    }

    async fn propose_graph_change(
        &self,
        request: AgentSessionRequest,
    ) -> Result<ValidatedAgentProposal, AgentError> {
        self.invocations.fetch_add(1, Ordering::SeqCst);
        self.graphs.lock().await.push(request.graph);
        Err(AgentError::Runtime(
            "proposal path is not expected in graph ingress tests".to_owned(),
        ))
    }
}
