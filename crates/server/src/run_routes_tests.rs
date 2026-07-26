use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use helixflow_graph::{GraphEdge, GraphNode, WorkflowGraph};
use helixflow_run::{AgentRunRequest, EventBus, SweepPlan, SweepVariant};
use helixflow_store::{NewVersion, Store, VersionSource};
use serde_json::json;

use crate::app_state::AppState;
use crate::graph_files::graph_hash;
use crate::run_routes::*;
use crate::test_support::FailingWorkbenchAgent;
use crate::test_wait::{wait_for_actual_cost, wait_for_run_status};

#[tokio::test]
async fn confirm_route_executes_waiting_run() {
    let (state, workspace_id, version_id, _dir) = state_with_workspace().await;
    let pending = state
        .runner
        .request_agent_run(AgentRunRequest {
            workspace_id: workspace_id.clone(),
            version_id,
            group_id: None,
            label: "Confirm run".to_owned(),
            provider: "mock".to_owned(),
            graph: sample_graph(),
        })
        .await
        .expect("request run");

    let response = confirm_run(
        Path((workspace_id.clone(), pending.run.id.clone())),
        State(state.clone()),
    )
    .await
    .expect("confirm response")
    .0;

    assert_eq!(response.run.status, "running");
    assert_eq!(response.pending_confirmation, None);
    wait_for_run_status(&state.store, &pending.run.id, "succeeded").await;
}

#[tokio::test]
async fn confirm_route_rejects_run_from_another_workspace() {
    let (state, _workspace_id, version_id, _dir) = state_with_workspace().await;
    let other = state
        .store
        .create_workspace("Other workspace")
        .await
        .expect("other workspace");
    let pending = state
        .runner
        .request_agent_run(AgentRunRequest {
            workspace_id: _workspace_id.clone(),
            version_id,
            group_id: None,
            label: "Confirm run".to_owned(),
            provider: "mock".to_owned(),
            graph: sample_graph(),
        })
        .await
        .expect("request run");

    let err = confirm_run(Path((other.id, pending.run.id)), State(state))
        .await
        .expect_err("workspace mismatch should fail");

    assert_eq!(err.status, axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn hold_route_interrupts_waiting_run() {
    let (state, workspace_id, version_id, _dir) = state_with_workspace().await;
    let pending = state
        .runner
        .request_agent_run(AgentRunRequest {
            workspace_id: workspace_id.clone(),
            version_id,
            group_id: None,
            label: "Hold run".to_owned(),
            provider: "mock".to_owned(),
            graph: sample_graph(),
        })
        .await
        .expect("request run");

    let response = hold_run(
        Path((workspace_id.clone(), pending.run.id.clone())),
        State(state.clone()),
    )
    .await
    .expect("hold response")
    .0;

    assert_eq!(response.run.status, "interrupted");
    assert_eq!(response.pending_confirmation, None);
    let run = state.store.run(&pending.run.id).await.expect("run");
    assert_eq!(run.status, "interrupted");
}

#[tokio::test]
async fn confirm_route_executes_sweep_group_and_selects_recommendation() {
    let (state, workspace_id, version_id, _dir) = state_with_workspace().await;
    let pending = state
        .runner
        .request_sweep_plan(SweepPlan {
            workspace_id: workspace_id.clone(),
            version_id,
            label: "Seed sweep".to_owned(),
            provider: "mock".to_owned(),
            variants: vec![
                SweepVariant {
                    label: "seed 101".to_owned(),
                    graph: executable_graph_with_duration(4),
                },
                SweepVariant {
                    label: "seed 202".to_owned(),
                    graph: executable_graph_with_duration(5),
                },
            ],
        })
        .await
        .expect("request sweep");
    let recommended_run_id = pending.runs[1].run.id.clone();

    let response = confirm_run(
        Path((workspace_id.clone(), recommended_run_id.clone())),
        State(state.clone()),
    )
    .await
    .expect("confirm sweep response")
    .0;

    assert_eq!(response.run.id, recommended_run_id);
    assert_eq!(response.run.status, "queued");
    assert!(response.outputs.is_empty());
    for pending_run in pending.runs {
        let run = wait_for_run_status(&state.store, &pending_run.run.id, "succeeded").await;
        wait_for_actual_cost(&state.store, &run.id).await;
        let ledger = state
            .store
            .cost_ledger_for_run(&run.id)
            .await
            .expect("cost ledger");
        assert!(ledger.iter().any(|entry| entry.estimated));
        assert!(ledger.iter().any(|entry| !entry.estimated));
    }
}

#[tokio::test]
async fn hold_route_interrupts_all_waiting_sweep_runs() {
    let (state, workspace_id, version_id, _dir) = state_with_workspace().await;
    let pending = state
        .runner
        .request_sweep_plan(SweepPlan {
            workspace_id: workspace_id.clone(),
            version_id,
            label: "Seed sweep".to_owned(),
            provider: "mock".to_owned(),
            variants: vec![
                SweepVariant {
                    label: "seed 101".to_owned(),
                    graph: executable_graph(),
                },
                SweepVariant {
                    label: "seed 202".to_owned(),
                    graph: executable_graph(),
                },
            ],
        })
        .await
        .expect("request sweep");
    let target_run_id = pending.runs[1].run.id.clone();

    let response = hold_run(
        Path((workspace_id.clone(), target_run_id.clone())),
        State(state.clone()),
    )
    .await
    .expect("hold sweep response")
    .0;

    assert_eq!(response.run.id, target_run_id);
    assert_eq!(response.run.status, "interrupted");
    assert_eq!(response.pending_confirmation, None);
    for pending_run in pending.runs {
        let run = state
            .store
            .run(&pending_run.run.id)
            .await
            .expect("sweep run");
        assert_eq!(run.status, "interrupted");
    }
}

#[tokio::test]
async fn queue_route_executes_current_workspace_graph() {
    let (state, workspace_id, _version_id, _dir) = state_with_workspace().await;

    let response = queue_workspace_run(Path(workspace_id.clone()), State(state.clone()), None)
        .await
        .expect("queue response")
        .0;

    assert_eq!(response.run.status, "running");
    assert_eq!(response.pending_confirmation, None);
    assert_eq!(response.run.steps.len(), 1);
    let run = wait_for_run_status(&state.store, &response.run.id, "succeeded").await;
    assert_eq!(run.trigger, "manual");
}

#[tokio::test]
async fn queue_route_rejects_empty_current_graph() {
    let (state, workspace_id, _version_id, _dir) = state_with_graph(&empty_graph()).await;

    let err = queue_workspace_run(Path(workspace_id), State(state), None)
        .await
        .expect_err("empty graph should not be queued");

    assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(err.message.contains("no executable steps"));
}

#[tokio::test]
async fn verified_read_queue_route_rejects_corrupt_graph_without_side_effects() {
    for payload in [
        StoredRunGraph::Missing,
        StoredRunGraph::InvalidStoredHash,
        StoredRunGraph::HashMismatch,
        StoredRunGraph::InvalidJson,
    ] {
        let (state, workspace_id, version_id, _dir) = state_with_run_payload(payload).await;
        let error = queue_workspace_run(Path(workspace_id.clone()), State(state.clone()), None)
            .await
            .expect_err("corrupt current graph must fail closed");

        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            !error
                .message
                .contains(state.data_dir.to_string_lossy().as_ref())
        );
        assert!(
            state
                .store
                .latest_workspace_run(&workspace_id)
                .await
                .expect("latest run")
                .is_none()
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
async fn queue_route_rejects_concurrent_in_flight_queue_request() {
    let (state, workspace_id, _version_id, _dir) = state_with_workspace().await;
    write_current_graph(&state).await;
    let _claim = claim_workspace_run_queue(&state, &workspace_id)
        .await
        .expect("claim queue");

    let err = queue_workspace_run(Path(workspace_id), State(state), None)
        .await
        .expect_err("second queue should fail while first is in flight");

    assert_eq!(err.status, StatusCode::CONFLICT);
    assert!(err.message.contains("queue request in flight"));
}

#[tokio::test]
async fn queue_route_rejects_waiting_confirmation_double_submit() {
    let (state, workspace_id, version_id, _dir) = state_with_workspace().await;
    write_current_graph(&state).await;
    let pending = state
        .runner
        .request_agent_run(AgentRunRequest {
            workspace_id: workspace_id.clone(),
            version_id,
            group_id: None,
            label: "Pending run".to_owned(),
            provider: "mock".to_owned(),
            graph: sample_graph(),
        })
        .await
        .expect("request run");

    let err = queue_workspace_run(Path(workspace_id), State(state), None)
        .await
        .expect_err("active run should block queue");

    assert_eq!(err.status, StatusCode::CONFLICT);
    assert!(err.message.contains(&pending.run.id));
}

#[tokio::test]
async fn interrupt_route_rejects_waiting_confirmation_run() {
    let (state, workspace_id, version_id, _dir) = state_with_workspace().await;
    let pending = state
        .runner
        .request_agent_run(AgentRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Pending run".to_owned(),
            provider: "mock".to_owned(),
            graph: sample_graph(),
        })
        .await
        .expect("request run");

    let err = interrupt_active_run(Path(pending.run.id), State(state))
        .await
        .expect_err("waiting confirmation is not active interrupt");

    assert_eq!(err.status, StatusCode::CONFLICT);
}

async fn state_with_workspace() -> (AppState, String, String, tempfile::TempDir) {
    state_with_graph(&sample_graph()).await
}

async fn state_with_graph(graph: &WorkflowGraph) -> (AppState, String, String, tempfile::TempDir) {
    state_with_run_payload(StoredRunGraph::Valid(graph)).await
}

#[derive(Clone, Copy)]
enum StoredRunGraph<'a> {
    Valid(&'a WorkflowGraph),
    Missing,
    InvalidStoredHash,
    HashMismatch,
    InvalidJson,
}

async fn state_with_run_payload(
    payload: StoredRunGraph<'_>,
) -> (AppState, String, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let data_dir = dir.path().to_path_buf();
    let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    let workspace = store
        .create_workspace("Run route workspace")
        .await
        .expect("create workspace");
    let canonical_bytes = serde_json::to_vec_pretty(&sample_graph()).expect("encode graph");
    let (file_bytes, stored_graph_hash) = match payload {
        StoredRunGraph::Valid(graph) => {
            let bytes = serde_json::to_vec_pretty(graph).expect("encode graph");
            let hash = graph_hash(&bytes);
            (Some(bytes), hash)
        }
        StoredRunGraph::Missing => (None, graph_hash(&canonical_bytes)),
        StoredRunGraph::InvalidStoredHash => {
            (Some(canonical_bytes), "sha256:not-canonical".to_owned())
        }
        StoredRunGraph::HashMismatch => (
            Some(serde_json::to_vec_pretty(&empty_graph()).expect("mismatch graph")),
            graph_hash(&canonical_bytes),
        ),
        StoredRunGraph::InvalidJson => {
            let bytes = b"invalid run graph json".to_vec();
            let hash = graph_hash(&bytes);
            (Some(bytes), hash)
        }
    };
    tokio::fs::create_dir_all(data_dir.join("graphs"))
        .await
        .expect("create graph dir");
    if let Some(file_bytes) = file_bytes {
        tokio::fs::write(data_dir.join("graphs/run.json"), file_bytes)
            .await
            .expect("write graph");
    }
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Run graph",
            source: VersionSource::Manual,
            graph_path: "graphs/run.json",
            graph_hash: &stored_graph_hash,
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("create version");
    let state = AppState::with_store_agent(
        EventBus::new(16),
        store,
        data_dir.clone(),
        Arc::new(FailingWorkbenchAgent),
        data_dir.join("sessions"),
    );
    (state, workspace.id, version.id, dir)
}

fn sample_graph() -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([(
            "text".to_owned(),
            GraphNode {
                node_type: "input.text".to_owned(),
                title: "Text".to_owned(),
                params: json!({ "text": "launch teaser" }),
                pos: [0.0, 0.0],
                size: None,
            },
        )]),
        edges: Vec::new(),
    }
}

fn empty_graph() -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::new(),
        edges: Vec::new(),
    }
}

fn executable_graph() -> WorkflowGraph {
    executable_graph_with_duration(4)
}

fn executable_graph_with_duration(duration_sec: u64) -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([
            (
                "video".to_owned(),
                GraphNode {
                    node_type: "video.text_to_video".to_owned(),
                    title: "Video render".to_owned(),
                    params: json!({
                        "prompt": "clean product shot",
                        "duration_sec": duration_sec,
                        "aspect_ratio": "9:16"
                    }),
                    pos: [0.0, 0.0],
                    size: None,
                },
            ),
            (
                "save".to_owned(),
                GraphNode {
                    node_type: "output.save".to_owned(),
                    title: "Save".to_owned(),
                    params: json!({}),
                    pos: [240.0, 0.0],
                    size: None,
                },
            ),
        ]),
        edges: vec![GraphEdge {
            from: ["video".to_owned(), "video".to_owned()],
            to: ["save".to_owned(), "artifact".to_owned()],
            edge_type: "artifact".to_owned(),
        }],
    }
}

async fn write_current_graph(state: &AppState) {
    write_graph(state, &sample_graph()).await;
}

async fn write_graph(state: &AppState, graph: &WorkflowGraph) {
    let graph_path = state.data_dir.join("graphs/run.json");
    tokio::fs::create_dir_all(graph_path.parent().expect("graph parent"))
        .await
        .expect("create graph dir");
    tokio::fs::write(
        graph_path,
        serde_json::to_vec_pretty(graph).expect("encode graph"),
    )
    .await
    .expect("write graph");
}
