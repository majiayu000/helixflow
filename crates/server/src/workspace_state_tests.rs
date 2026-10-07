use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use helixflow_gateway::RuntimeProvider;
use helixflow_graph::{GraphEdge, GraphNode, WorkflowGraph};
use helixflow_registry::NodeRegistry;
use helixflow_run::EventBus;
use helixflow_store::{
    NewMessage, NewProposal, NewRun, NewRunStep, NewVersion, Store, VersionSource,
};
use serde_json::{Value, json};
use std::path::Path;
use std::path::PathBuf;

use crate::app_state::AppState;
use crate::graph_files::graph_hash;
use crate::test_support::FailingWorkbenchAgent;
use crate::workspace_state::*;

#[tokio::test]
async fn workspace_state_uses_store_records_and_graph_file() {
    let (state, workspace_id, _dir) = state_with_workspace().await;
    state
        .store
        .create_message(NewMessage {
            workspace_id: &workspace_id,
            role: "user",
            kind: "text",
            text: Some("Build this"),
            ref_id: None,
            attachment_ids_json: Some(r#"{"turnMode":"modify_workflow"}"#),
            conversation_id: None,
            turn_id: None,
        })
        .await
        .expect("create message");

    let body = workspace_state(AxumPath(workspace_id.clone()), State(state))
        .await
        .expect("workspace state")
        .0;

    assert_eq!(body["workspace"]["id"], workspace_id);
    assert_eq!(body["workspace"]["name"], "Store workspace");
    assert_eq!(body["chat"]["messages"][0]["text"], "Build this");
    assert_eq!(body["chat"]["messages"][0]["turnMode"], "modify_workflow");
    assert_eq!(body["graph"]["nodes"].as_array().expect("nodes").len(), 1);
    assert_eq!(body["providers"]["defaultProvider"], "mock");
    assert_eq!(body["providers"]["runtimeProviders"][0]["id"], "mock");
    assert_eq!(
        body["providers"]["runtimeProviders"][0]["kind"],
        "local_test"
    );
    assert_eq!(
        body["providers"]["runtimeProviders"][0]["status"],
        "healthy"
    );
    assert_eq!(body["providers"]["apiConnectors"][0]["provider"], "mock");
    assert_eq!(body["run"], Value::Null);
    assert!(body["pendingConfirmation"].is_null());
    assert_ne!(body["workspace"]["name"], "Helixflow Demo");
}

#[tokio::test]
async fn confirmation_reload_uses_the_persisted_known_estimate() {
    let (state, workspace_id, _dir) = state_with_workspace().await;
    let version_id = state
        .store
        .workspace(&workspace_id)
        .await
        .expect("workspace")
        .cur_version_id
        .expect("version");
    state
        .store
        .create_run(NewRun {
            workspace_id: &workspace_id,
            version_id: &version_id,
            group_id: None,
            label: "Paid render",
            trigger: "manual",
            plan_json: None,
            estimate_json: Some(
                r#"{"amount":1.25,"currency":"USD","estimated":true,"unknown":false}"#,
            ),
            status: "waiting_confirmation",
        })
        .await
        .expect("waiting run");

    let body = workspace_state_value(&state, &workspace_id)
        .await
        .expect("reload");
    assert_eq!(body["pendingConfirmation"]["cost"]["amount"], 1.25);
    assert_eq!(body["pendingConfirmation"]["cost"]["currency"], "USD");
}

#[tokio::test]
async fn confirmation_reload_keeps_absent_estimates_unknown_and_rejects_malformed_json() {
    for estimate_json in [None, Some("invalid estimate JSON")] {
        let (state, workspace_id, _dir) = state_with_workspace().await;
        let version_id = state
            .store
            .workspace(&workspace_id)
            .await
            .expect("workspace")
            .cur_version_id
            .expect("version");
        state
            .store
            .create_run(NewRun {
                workspace_id: &workspace_id,
                version_id: &version_id,
                group_id: None,
                label: "Corrupt estimate",
                trigger: "manual",
                plan_json: None,
                estimate_json,
                status: "waiting_confirmation",
            })
            .await
            .expect("waiting run");

        let response = workspace_state_value(&state, &workspace_id).await;
        if estimate_json.is_none() {
            let body = response.expect("a run without an estimate is still visible");
            assert!(!body["pendingConfirmation"].is_null());
            assert_eq!(body["pendingConfirmation"]["cost"]["amount"], Value::Null);
        } else {
            let error = response.expect_err("malformed cost estimate must fail loudly");
            assert_eq!(error.status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
            assert!(error.message.contains("cost estimate"));
        }
    }
}

#[test]
fn graph_payload_populates_provider_from_node_registry_before_run_steps() {
    let graph = WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([(
            "video".to_owned(),
            GraphNode {
                node_type: "video.text_to_video".to_owned(),
                title: "Video".to_owned(),
                params: json!({
                    "prompt": "clean product shot",
                    "duration_sec": 4,
                    "aspect_ratio": "9:16"
                }),
                pos: [10.0, 20.0],
                size: Some([260.0, 180.0]),
                semantics: None,
            },
        )]),
        edges: Vec::new(),
        catalog_revision: None,
    };

    let body = graph_payload(&graph, &BTreeMap::new(), &NodeRegistry::builtin(), "mock");

    assert_eq!(body["nodes"][0]["provider"], "mock");
    assert_eq!(body["nodes"][0]["status"], "queued");
    assert_eq!(body["nodes"][0]["size"]["width"], 260.0);
    assert_eq!(body["nodes"][0]["size"]["height"], 180.0);
}

#[tokio::test]
async fn workspace_state_includes_unavailable_provider_without_mock_fallback() -> Result<(), String>
{
    let dir = tempfile::tempdir().map_err(|err| err.to_string())?;
    let store = open_store(dir.path()).await;
    let workspace = store
        .create_workspace("Unavailable provider workspace")
        .await
        .map_err(|err| err.to_string())?;
    let state = test_state_with_provider(
        store,
        dir.path().to_path_buf(),
        RuntimeProvider::unavailable(
            "openai",
            "runtime provider `openai` is not configured by this build",
        ),
    );

    let body = workspace_state(AxumPath(workspace.id), State(state))
        .await
        .map_err(|err| err.message)?
        .0;

    assert_eq!(body["providers"]["defaultProvider"], "openai");
    assert_eq!(body["providers"]["runtimeProviders"][0]["id"], "openai");
    assert_eq!(
        body["providers"]["runtimeProviders"][0]["kind"],
        "unavailable"
    );
    assert_eq!(
        body["providers"]["runtimeProviders"][0]["status"],
        "unavailable"
    );
    assert_eq!(body["providers"]["runtimeProviders"][0]["enabled"], false);
    assert_eq!(
        body["providers"]["runtimeProviders"][0]["capabilities"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(
        body["providers"]["workflowBackends"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
    assert_eq!(
        body["providers"]["apiConnectors"].as_array().map(Vec::len),
        Some(0)
    );
    Ok(())
}

#[tokio::test]
async fn workspace_state_returns_blank_graph_without_current_version() {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = open_store(dir.path()).await;
    let workspace = store
        .create_workspace("Empty workspace")
        .await
        .expect("create workspace");
    let state = test_state(store, dir.path().to_path_buf());

    let body = workspace_state(AxumPath(workspace.id), State(state))
        .await
        .expect("workspace state")
        .0;

    assert_eq!(body["graph"]["nodes"].as_array().expect("nodes").len(), 0);
    assert_eq!(
        body["workflowGraph"]["nodes"]
            .as_object()
            .expect("nodes")
            .len(),
        0
    );
    assert!(body["run"].is_null());
}

#[tokio::test]
async fn workspace_state_includes_failed_run_error_payload() {
    let (state, workspace_id, _dir) = state_with_workspace().await;
    let version_id = state
        .store
        .workspace(&workspace_id)
        .await
        .expect("workspace")
        .cur_version_id
        .expect("current version");
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
            node_id: "input",
            node_type: "input.text",
            provider: Some("mock"),
            state: "running",
        })
        .await
        .expect("create step");
    let error_json =
        r#"{"error":"provider rejected duration","trace":"stack line 1\nstack line 2"}"#;
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

    let body = workspace_state(AxumPath(workspace_id), State(state))
        .await
        .expect("workspace state")
        .0;

    assert_eq!(body["run"]["status"], "failed");
    assert_eq!(
        body["run"]["error"]["summary"],
        "provider rejected duration"
    );
    assert!(
        body["run"]["error"]["raw"]
            .as_str()
            .expect("raw")
            .contains("stack line")
    );
    assert_eq!(
        body["run"]["steps"][0]["error"]["summary"],
        "provider rejected duration"
    );
    assert_eq!(body["graph"]["nodes"][0]["status"], "failed");
}

#[tokio::test]
async fn workspace_state_marks_cached_run_steps_and_graph_nodes() {
    let (state, workspace_id, _dir) = state_with_workspace().await;
    let version_id = state
        .store
        .workspace(&workspace_id)
        .await
        .expect("workspace")
        .cur_version_id
        .expect("current version");
    let run = state
        .store
        .create_run(NewRun {
            workspace_id: &workspace_id,
            version_id: &version_id,
            group_id: None,
            label: "Cached render",
            trigger: "manual",
            plan_json: None,
            estimate_json: None,
            status: "succeeded",
        })
        .await
        .expect("create run");
    let step = state
        .store
        .create_run_step(NewRunStep {
            run_id: &run.id,
            node_id: "input",
            node_type: "input.text",
            provider: None,
            state: "queued",
        })
        .await
        .expect("create step");
    state
        .store
        .update_run_step_state(&step.id, "succeeded", Some(1.0), None, None)
        .await
        .expect("complete step before attaching cache metadata");
    state
        .store
        .mark_run_step_cached_succeeded(&step.id, r#"{"cached":true}"#)
        .await
        .expect("mark cached");

    let body = workspace_state(AxumPath(workspace_id), State(state))
        .await
        .expect("workspace state")
        .0;

    assert_eq!(body["run"]["steps"][0]["cached"], true);
    assert_eq!(body["graph"]["nodes"][0]["cached"], true);
}

#[tokio::test]
async fn verified_read_workspace_state_rejects_corrupt_graph_without_side_effects() {
    for payload in [
        StoredStateGraph::Missing,
        StoredStateGraph::InvalidStoredHash,
        StoredStateGraph::HashMismatch,
        StoredStateGraph::InvalidJson,
    ] {
        let (state, workspace_id, version_id, _dir) = state_with_state_payload(payload).await;
        let error = workspace_state(AxumPath(workspace_id.clone()), State(state.clone()))
            .await
            .expect_err("corrupt current graph must fail closed");

        assert_eq!(error.status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            !error
                .message
                .contains(state.data_dir.to_string_lossy().as_ref())
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
        assert!(
            state
                .store
                .latest_workspace_run(&workspace_id)
                .await
                .expect("latest run")
                .is_none()
        );
        let versions = state
            .store
            .versions_for_workspace(&workspace_id)
            .await
            .expect("versions");
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].id, version_id);
        assert_eq!(
            state
                .store
                .workspace(&workspace_id)
                .await
                .expect("workspace")
                .cur_version_id
                .as_deref(),
            Some(version_id.as_str())
        );
    }
}

#[tokio::test]
async fn persisted_pending_proposal_reloads_parameter_diff_without_values() {
    let (state, workspace_id, dir) = state_with_workspace().await;
    let base_version_id = state
        .store
        .workspace(&workspace_id)
        .await
        .expect("workspace")
        .cur_version_id
        .expect("current version");
    persist_parameter_proposal(&state, &workspace_id, &base_version_id).await;
    drop(state);
    let state = test_state(open_store(dir.path()).await, dir.path().to_path_buf());

    let body = workspace_state_value(&state, &workspace_id)
        .await
        .expect("reload workspace state");

    assert_eq!(
        body["pendingProposal"]["diffSummary"],
        json!(["~ /nodes/input/params/summary"])
    );
    assert_eq!(body["pendingProposal"]["baseVersionId"], base_version_id);
    let summary = body["pendingProposal"]["diffSummary"].to_string();
    assert!(!summary.contains("Source text"));
    assert!(!summary.contains("private-example-value"));
}

#[tokio::test]
async fn stale_pending_proposal_diff_uses_its_base_instead_of_current_graph() {
    let (state, workspace_id, _dir) = state_with_workspace().await;
    let base_version_id = state
        .store
        .workspace(&workspace_id)
        .await
        .expect("workspace")
        .cur_version_id
        .expect("current version");
    persist_parameter_proposal(&state, &workspace_id, &base_version_id).await;
    advance_current_graph(&state, &workspace_id, &base_version_id).await;

    let body = workspace_state_value(&state, &workspace_id)
        .await
        .expect("workspace state with stale proposal");

    assert_ne!(body["workspace"]["versionId"], base_version_id);
    assert_eq!(
        body["pendingProposal"]["diffSummary"],
        json!(["~ /nodes/input/params/summary"])
    );
}

#[tokio::test]
async fn stale_pending_proposal_rejects_corrupt_historical_base() {
    let (state, workspace_id, _dir) = state_with_workspace().await;
    let base_version_id = state
        .store
        .workspace(&workspace_id)
        .await
        .expect("workspace")
        .cur_version_id
        .expect("current version");
    persist_parameter_proposal(&state, &workspace_id, &base_version_id).await;
    advance_current_graph(&state, &workspace_id, &base_version_id).await;
    tokio::fs::write(state.data_dir.join("graphs/current.json"), b"{}")
        .await
        .expect("corrupt historical base");

    let error = workspace_state_value(&state, &workspace_id)
        .await
        .expect_err("historical base hash mismatch must fail closed");

    assert_eq!(error.status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    assert!(error.message.contains("mismatched version_graph hash"));
    assert!(
        !error
            .message
            .contains(state.data_dir.to_string_lossy().as_ref())
    );
    assert_eq!(
        state
            .store
            .workspace_proposals(&workspace_id)
            .await
            .expect("proposals")[0]
            .state,
        "pending"
    );
}

#[tokio::test]
async fn pending_proposal_rejects_base_from_another_workspace() {
    let (state, workspace_id, _dir) = state_with_workspace().await;
    let other_workspace = state
        .store
        .create_workspace("Other workspace")
        .await
        .expect("other workspace");
    let graph_bytes = serde_json::to_vec(&sample_graph()).expect("graph json");
    let foreign_base = state
        .store
        .create_version(NewVersion {
            workspace_id: &other_workspace.id,
            label: "Foreign base",
            source: VersionSource::Manual,
            graph_path: "graphs/current.json",
            graph_hash: &graph_hash(&graph_bytes),
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("foreign version");
    persist_parameter_proposal(&state, &workspace_id, &foreign_base.id).await;

    let error = workspace_state_value(&state, &workspace_id)
        .await
        .expect_err("foreign base must fail closed");

    assert_eq!(error.status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        error.message,
        "pending proposal base version belongs to another workspace"
    );
}

async fn persist_parameter_proposal(state: &AppState, workspace_id: &str, base_version_id: &str) {
    let mut preview = sample_graph();
    preview.nodes.get_mut("input").expect("input node").params["summary"] =
        json!("private-example-value");
    let ops = vec![helixflow_graph::ProposalOp::SetParam {
        id: "input".to_owned(),
        key: "summary".to_owned(),
        prev: Some(json!("Source text")),
        value: json!("private-example-value"),
    }];
    tokio::fs::write(
        state.data_dir.join("graphs/proposal-ops.json"),
        serde_json::to_vec(&ops).expect("ops json"),
    )
    .await
    .expect("write proposal ops");
    tokio::fs::write(
        state.data_dir.join("graphs/proposal-preview.json"),
        serde_json::to_vec(&preview).expect("preview json"),
    )
    .await
    .expect("write proposal preview");
    state
        .store
        .create_proposal(NewProposal {
            workspace_id,
            base_version_id,
            kind: "modify",
            title: "Change input text",
            summary: "Update the source text",
            ops_path: "graphs/proposal-ops.json",
            preview_graph_path: Some("graphs/proposal-preview.json"),
            message_id: None,
        })
        .await
        .expect("persist proposal");
}

async fn advance_current_graph(state: &AppState, workspace_id: &str, base_version_id: &str) {
    let mut current = sample_graph();
    let node = current.nodes.get_mut("input").expect("input node");
    node.params["summary"] = json!("private-example-value");
    node.title = "Later title".to_owned();
    let graph_bytes = serde_json::to_vec(&current).expect("current graph json");
    tokio::fs::write(state.data_dir.join("graphs/later.json"), &graph_bytes)
        .await
        .expect("write later graph");
    state
        .store
        .create_version_after(
            NewVersion {
                workspace_id,
                label: "Later graph",
                source: VersionSource::Manual,
                graph_path: "graphs/later.json",
                graph_hash: &graph_hash(&graph_bytes),
                parent_id: Some(base_version_id),
                semantics_json: None,
            },
            base_version_id,
        )
        .await
        .expect("advance current graph");
}

async fn state_with_workspace() -> (AppState, String, tempfile::TempDir) {
    let (state, workspace_id, _version_id, dir) =
        state_with_state_payload(StoredStateGraph::Valid).await;
    (state, workspace_id, dir)
}

#[derive(Clone, Copy)]
enum StoredStateGraph {
    Valid,
    Missing,
    InvalidStoredHash,
    HashMismatch,
    InvalidJson,
}

async fn state_with_state_payload(
    payload: StoredStateGraph,
) -> (AppState, String, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let data_dir = dir.path().to_path_buf();
    let store = open_store(&data_dir).await;
    let workspace = store
        .create_workspace("Store workspace")
        .await
        .expect("create workspace");
    tokio::fs::create_dir_all(data_dir.join("graphs"))
        .await
        .expect("create graph dir");
    let graph_path = "graphs/current.json";
    let canonical_bytes = serde_json::to_vec(&sample_graph()).expect("graph json");
    let (file_bytes, stored_graph_hash) = match payload {
        StoredStateGraph::Valid => (Some(canonical_bytes.clone()), graph_hash(&canonical_bytes)),
        StoredStateGraph::Missing => (None, graph_hash(&canonical_bytes)),
        StoredStateGraph::InvalidStoredHash => {
            (Some(canonical_bytes), "sha256:not-canonical".to_owned())
        }
        StoredStateGraph::HashMismatch => (Some(b"{}".to_vec()), graph_hash(&canonical_bytes)),
        StoredStateGraph::InvalidJson => {
            let bytes = b"invalid workspace graph json".to_vec();
            let hash = graph_hash(&bytes);
            (Some(bytes), hash)
        }
    };
    if let Some(file_bytes) = file_bytes {
        tokio::fs::write(data_dir.join(graph_path), file_bytes)
            .await
            .expect("write graph");
    }
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Current graph",
            source: VersionSource::Manual,
            graph_path,
            graph_hash: &stored_graph_hash,
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("create version");

    let state = test_state(store, data_dir);
    (state, workspace.id, version.id, dir)
}

async fn open_store(data_dir: &Path) -> Store {
    let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
    Store::open(&database_url).await.expect("open store")
}

fn test_state(store: Store, data_dir: PathBuf) -> AppState {
    AppState::with_store_agent(
        EventBus::new(16),
        store,
        data_dir.clone(),
        Arc::new(FailingWorkbenchAgent),
        data_dir.join("sessions"),
    )
}

fn test_state_with_provider(
    store: Store,
    data_dir: PathBuf,
    provider: RuntimeProvider,
) -> AppState {
    AppState::with_store_agent_provider(
        EventBus::new(16),
        store,
        data_dir.clone(),
        Arc::new(FailingWorkbenchAgent),
        data_dir.join("sessions"),
        provider,
    )
}

fn sample_graph() -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([(
            "input".to_owned(),
            GraphNode {
                node_type: "input.text".to_owned(),
                title: "Input".to_owned(),
                params: json!({ "summary": "Source text" }),
                pos: [10.0, 20.0],
                size: None,
                semantics: None,
            },
        )]),
        edges: vec![GraphEdge {
            from: ["input".to_owned(), "text".to_owned()],
            to: ["input".to_owned(), "text".to_owned()],
            edge_type: "text".to_owned(),
        }],
        catalog_revision: None,
    }
}
