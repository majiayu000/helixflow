use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::extract::{Path, State};
use helixflow_agent::{
    AgentError, AgentSessionRequest, ValidatedAgentProposal, ValidatedAgentReply,
};
use helixflow_graph::{GraphEdge, GraphNode, WorkflowGraph};
use helixflow_run::{EventBus, ManualRunRequest};
use helixflow_store::{NewVersion, Store, VersionSource};
use serde_json::json;

use crate::app_state::{AppState, WorkbenchAgent};
use crate::artifact_routes::{RejectOutputRequest, reject_output};
use crate::graph_files::graph_hash;

#[tokio::test]
async fn reject_with_rerun_executes_provider_bypasses_cache_and_records_actual_cost() {
    let (state, workspace_id, version_id, _dir) = review_state().await;
    let graph = review_graph();
    let parent = state
        .runner
        .prepare_manual_run(ManualRunRequest {
            workspace_id: workspace_id.clone(),
            version_id,
            group_id: None,
            label: "Review parent".to_owned(),
            provider: "mock".to_owned(),
            graph,
            force_rerun: false,
        })
        .await
        .expect("prepare parent");
    state
        .runner
        .start_confirmed_run(&parent.run.id)
        .await
        .expect("start parent");
    wait_for_review_status(&state.store, &parent.run.id, "succeeded").await;
    let parent_artifacts = state
        .store
        .run_artifacts(&parent.run.id)
        .await
        .expect("parent artifacts");
    let artifact = parent_artifacts
        .iter()
        .find(|artifact| artifact.kind == "video")
        .or_else(|| parent_artifacts.first())
        .expect("reviewable output")
        .clone();
    let mut events = state.events.subscribe();

    let _ = reject_output(
        Path(artifact.id.clone()),
        State(state.clone()),
        axum::Json(RejectOutputRequest { rerun: true }),
    )
    .await
    .expect("reject and rerun");
    let child = wait_for_review_retry(&state.store, &workspace_id).await;
    wait_for_review_status(&state.store, &child.id, "succeeded").await;

    assert_eq!(child.parent_run_id.as_deref(), Some(parent.run.id.as_str()));
    assert!(child.force_rerun);
    let steps = state.store.run_steps(&child.id).await.expect("child steps");
    assert!(
        steps
            .iter()
            .all(|step| !step_was_cached(step.metadata_json.as_deref()))
    );
    let ledger = state
        .store
        .cost_ledger_for_run(&child.id)
        .await
        .expect("child ledger");
    assert!(ledger.iter().any(|entry| !entry.estimated));
    assert!(
        !state
            .store
            .run_artifacts(&child.id)
            .await
            .expect("child artifacts")
            .is_empty()
    );

    let retry_event = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.expect("event");
            if event.run_id == parent.run.id && event.ev == "run.retry" {
                return event;
            }
        }
    })
    .await
    .expect("run.retry event timeout");
    assert_eq!(retry_event.data["child_run_id"], child.id);
    assert_eq!(retry_event.data["reason"], "output_rejected");
}

fn step_was_cached(metadata_json: Option<&str>) -> bool {
    metadata_json
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|value| value.get("cached").and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

async fn review_state() -> (AppState, String, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let data_dir = dir.path().to_path_buf();
    let store = Store::open(&format!(
        "sqlite://{}",
        data_dir.join("review.sqlite").display()
    ))
    .await
    .expect("store");
    let workspace = store
        .create_workspace("Review retry")
        .await
        .expect("workspace");
    let graph_path = PathBuf::from("graphs/review.json");
    tokio::fs::create_dir_all(data_dir.join("graphs"))
        .await
        .expect("graph directory");
    let graph_bytes = serde_json::to_vec_pretty(&review_graph()).expect("graph json");
    let stored_graph_hash = graph_hash(&graph_bytes);
    tokio::fs::write(data_dir.join(&graph_path), graph_bytes)
        .await
        .expect("write graph");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Review graph",
            source: VersionSource::Manual,
            graph_path: graph_path.to_str().expect("graph path"),
            graph_hash: &stored_graph_hash,
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("version");
    let events = EventBus::new(64);
    let state = AppState::with_store_agent(
        events,
        store,
        data_dir.clone(),
        Arc::new(UnusedAgent),
        data_dir.join("sessions"),
    );
    (state, workspace.id, version.id, dir)
}

fn review_graph() -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([
            (
                "video".to_owned(),
                GraphNode {
                    node_type: "video.text_to_video".to_owned(),
                    title: "Video".to_owned(),
                    params: json!({ "prompt": "review", "duration_sec": 4, "aspect_ratio": "9:16" }),
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

async fn wait_for_review_retry(store: &Store, workspace_id: &str) -> helixflow_store::RunRecord {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(run) = store
                .latest_workspace_run(workspace_id)
                .await
                .expect("latest")
                && run.attempt > 0
            {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("retry timeout")
}

async fn wait_for_review_status(store: &Store, run_id: &str, expected: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if store.run(run_id).await.expect("run").status == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("status timeout");
}

struct UnusedAgent;

#[async_trait]
impl WorkbenchAgent for UnusedAgent {
    async fn answer_chat(
        &self,
        _request: AgentSessionRequest,
    ) -> Result<ValidatedAgentReply, AgentError> {
        Err(AgentError::Runtime(
            "agent is unused in artifact retry tests".to_owned(),
        ))
    }

    async fn propose_graph_change(
        &self,
        _request: AgentSessionRequest,
    ) -> Result<ValidatedAgentProposal, AgentError> {
        Err(AgentError::Runtime(
            "agent is unused in artifact retry tests".to_owned(),
        ))
    }
}
