use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use helixflow_gateway::RuntimeProvider;
use helixflow_graph::{GraphEdge, GraphNode, WorkflowGraph};
use helixflow_registry::NodeRegistry;
use helixflow_run::EventBus;
use helixflow_store::{NewMessage, NewRun, NewRunStep, NewVersion, Store, VersionSource};
use serde_json::{Value, json};
use std::path::Path;
use std::path::PathBuf;

use crate::app_state::AppState;
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
                size: None,
            },
        )]),
        edges: Vec::new(),
    };

    let body = graph_payload(&graph, &BTreeMap::new(), &NodeRegistry::builtin(), "mock");

    assert_eq!(body["nodes"][0]["provider"], "mock");
    assert_eq!(body["nodes"][0]["status"], "queued");
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
async fn workspace_state_rejects_missing_current_graph_file() {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = open_store(dir.path()).await;
    let workspace = store
        .create_workspace("Broken workspace")
        .await
        .expect("create workspace");
    store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Missing graph",
            source: VersionSource::Manual,
            graph_path: "graphs/missing.json",
            graph_hash: "sha256:missing",
            parent_id: None,
        })
        .await
        .expect("create version");
    let state = test_state(store, dir.path().to_path_buf());

    let err = workspace_state(AxumPath(workspace.id), State(state))
        .await
        .expect_err("missing graph file should error");

    assert_eq!(err.status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    assert!(err.message.contains("graphs/missing.json"));
}

async fn state_with_workspace() -> (AppState, String, tempfile::TempDir) {
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
    tokio::fs::write(
        data_dir.join(graph_path),
        serde_json::to_vec(&sample_graph()).expect("graph json"),
    )
    .await
    .expect("write graph");
    store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Current graph",
            source: VersionSource::Manual,
            graph_path,
            graph_hash: "sha256:current",
            parent_id: None,
        })
        .await
        .expect("create version");

    let state = test_state(store, data_dir);
    (state, workspace.id, dir)
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
            },
        )]),
        edges: vec![GraphEdge {
            from: ["input".to_owned(), "text".to_owned()],
            to: ["input".to_owned(), "text".to_owned()],
            edge_type: "text".to_owned(),
        }],
    }
}
