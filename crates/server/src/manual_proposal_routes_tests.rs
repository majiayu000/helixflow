use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use helixflow_agent::{
    AgentError, AgentSessionRequest, ValidatedAgentProposal, ValidatedAgentReply,
};
use helixflow_graph::{GraphEdge, GraphNode, WorkflowGraph};
use helixflow_run::EventBus;
use helixflow_store::{NewVersion, Store, VersionSource};
use serde_json::json;

use crate::app_state::{AppState, WorkbenchAgent};
use crate::manual_proposal_routes::{
    ManualProposalOpRequest, ManualProposalRequest, create_manual_workspace_proposal,
};

#[tokio::test]
async fn manual_set_param_creates_pending_proposal_without_changing_current_version() {
    let (state, workspace_id, version_id, _dir) =
        manual_proposal_state_with_graph(manual_route_sample_graph()).await;

    let body = create_manual_workspace_proposal(
        Path(workspace_id.clone()),
        State(state.clone()),
        Json(manual_route_request(
            &version_id,
            ManualProposalOpRequest::SetParam {
                id: "video".to_owned(),
                key: "duration_sec".to_owned(),
                value: json!(4),
            },
        )),
    )
    .await
    .expect("manual proposal")
    .0;

    assert_eq!(body["workspace"]["versionId"], version_id);
    assert_eq!(
        body["workflowGraph"]["nodes"]["video"]["params"]["duration_sec"],
        5
    );
    assert_eq!(body["pendingProposal"]["state"], "pending");
    assert_eq!(
        body["pendingProposal"]["previewGraph"]["nodes"]["video"]["params"]["duration_sec"],
        4
    );
    assert_eq!(body["pendingProposal"]["ops"][0]["op"], "set_param");
    assert_eq!(body["pendingProposal"]["ops"][0]["prev"], json!(5));
    assert!(
        body["pendingProposal"]["diffSummary"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );

    let proposals = state
        .store
        .workspace_proposals(&workspace_id)
        .await
        .expect("proposals");
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].state, "pending");
}

#[tokio::test]
async fn manual_add_remove_and_edge_ops_create_pending_proposals() {
    let add_node = ManualProposalOpRequest::AddNode {
        id: "extra_text".to_owned(),
        node_type: "input.text".to_owned(),
        title: None,
        params: json!({ "text": "manual input" }),
        pos: [20.0, 320.0],
    };
    assert_manual_success(add_node, |body| {
        assert!(body["pendingProposal"]["previewGraph"]["nodes"]["extra_text"].is_object());
    })
    .await;

    let remove_node = ManualProposalOpRequest::RemoveNode {
        id: "writer".to_owned(),
    };
    assert_manual_success(remove_node, |body| {
        assert!(body["pendingProposal"]["previewGraph"]["nodes"]["writer"].is_null());
    })
    .await;

    let add_edge = ManualProposalOpRequest::AddEdge {
        from: ["writer".to_owned(), "prompt".to_owned()],
        to: ["video".to_owned(), "prompt".to_owned()],
        edge_type: "text".to_owned(),
    };
    assert_manual_success_with_graph(graph_without_video_edge(), add_edge, |body| {
        assert_eq!(
            body["pendingProposal"]["previewGraph"]["edges"]
                .as_array()
                .expect("edges")
                .len(),
            2
        );
    })
    .await;

    let remove_edge = ManualProposalOpRequest::RemoveEdge {
        from: ["writer".to_owned(), "prompt".to_owned()],
        to: ["video".to_owned(), "prompt".to_owned()],
        edge_type: "text".to_owned(),
    };
    assert_manual_success(remove_edge, |body| {
        assert_eq!(
            body["pendingProposal"]["previewGraph"]["edges"]
                .as_array()
                .expect("edges")
                .len(),
            1
        );
    })
    .await;
}

#[tokio::test]
async fn manual_proposal_rejects_invalid_params_without_creating_pending_proposal() {
    let (state, workspace_id, version_id, _dir) =
        manual_proposal_state_with_graph(manual_route_sample_graph()).await;

    let err = create_manual_workspace_proposal(
        Path(workspace_id.clone()),
        State(state.clone()),
        Json(manual_route_request(
            &version_id,
            ManualProposalOpRequest::SetParam {
                id: "video".to_owned(),
                key: "duration_sec".to_owned(),
                value: json!("slow"),
            },
        )),
    )
    .await
    .expect_err("invalid params should fail");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
    assert!(err.message.contains("expected integer"));
    assert!(
        state
            .store
            .workspace_proposals(&workspace_id)
            .await
            .expect("proposals")
            .is_empty()
    );
}

#[tokio::test]
async fn manual_proposal_rejects_stale_base_version_and_existing_pending() {
    let (state, workspace_id, version_id, _dir) =
        manual_proposal_state_with_graph(manual_route_sample_graph()).await;
    let stale = create_manual_workspace_proposal(
        Path(workspace_id.clone()),
        State(state.clone()),
        Json(manual_route_request(
            "ver_old",
            ManualProposalOpRequest::SetParam {
                id: "video".to_owned(),
                key: "duration_sec".to_owned(),
                value: json!(4),
            },
        )),
    )
    .await
    .expect_err("stale base should fail");
    assert_eq!(stale.status, StatusCode::CONFLICT);

    let first = create_manual_workspace_proposal(
        Path(workspace_id.clone()),
        State(state.clone()),
        Json(manual_route_request(
            &version_id,
            ManualProposalOpRequest::SetParam {
                id: "video".to_owned(),
                key: "duration_sec".to_owned(),
                value: json!(4),
            },
        )),
    )
    .await
    .expect("first pending proposal")
    .0;
    assert_eq!(first["pendingProposal"]["state"], "pending");

    let second = create_manual_workspace_proposal(
        Path(workspace_id.clone()),
        State(state.clone()),
        Json(manual_route_request(
            &version_id,
            ManualProposalOpRequest::SetParam {
                id: "video".to_owned(),
                key: "duration_sec".to_owned(),
                value: json!(3),
            },
        )),
    )
    .await
    .expect_err("second pending proposal should fail");
    assert_eq!(second.status, StatusCode::BAD_REQUEST);
    assert!(second.message.contains("already has a pending proposal"));
}

async fn assert_manual_success(
    op: ManualProposalOpRequest,
    assert_body: impl FnOnce(&serde_json::Value),
) {
    assert_manual_success_with_graph(manual_route_sample_graph(), op, assert_body).await;
}

async fn assert_manual_success_with_graph(
    graph: WorkflowGraph,
    op: ManualProposalOpRequest,
    assert_body: impl FnOnce(&serde_json::Value),
) {
    let (state, workspace_id, version_id, _dir) = manual_proposal_state_with_graph(graph).await;
    let body = create_manual_workspace_proposal(
        Path(workspace_id),
        State(state),
        Json(manual_route_request(&version_id, op)),
    )
    .await
    .expect("manual proposal")
    .0;

    assert_eq!(body["workspace"]["versionId"], version_id);
    assert_body(&body);
}

fn manual_route_request(
    base_version_id: &str,
    op: ManualProposalOpRequest,
) -> ManualProposalRequest {
    ManualProposalRequest {
        base_version_id: base_version_id.to_owned(),
        title: None,
        summary: None,
        op,
    }
}

async fn manual_proposal_state_with_graph(
    graph: WorkflowGraph,
) -> (AppState, String, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let data_dir = dir.path().to_path_buf();
    let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    let workspace = store
        .create_workspace("Manual proposal route workspace")
        .await
        .expect("create workspace");
    let graph_path = PathBuf::from("workspaces")
        .join(&workspace.id)
        .join("graphs")
        .join("base.json");
    tokio::fs::create_dir_all(data_dir.join(graph_path.parent().expect("graph parent")))
        .await
        .expect("create graph dir");
    tokio::fs::write(
        data_dir.join(&graph_path),
        serde_json::to_vec_pretty(&graph).expect("graph json"),
    )
    .await
    .expect("write graph");
    let graph_path_string = graph_path.to_string_lossy().into_owned();
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Base graph",
            source: VersionSource::Manual,
            graph_path: &graph_path_string,
            graph_hash: "sha256:base",
            parent_id: None,
        })
        .await
        .expect("create version");
    let state = AppState::with_store_agent(
        EventBus::new(16),
        store,
        data_dir.clone(),
        Arc::new(ManualRouteNoopWorkbenchAgent),
        data_dir.join("sessions"),
    );
    (state, workspace.id, version.id, dir)
}

fn graph_without_video_edge() -> WorkflowGraph {
    let mut graph = manual_route_sample_graph();
    graph.edges.pop();
    graph
}

fn manual_route_sample_graph() -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([
            (
                "input".to_owned(),
                GraphNode {
                    node_type: "input.text".to_owned(),
                    title: "Text".to_owned(),
                    params: json!({ "text": "make a product clip" }),
                    pos: [0.0, 0.0],
                },
            ),
            (
                "writer".to_owned(),
                GraphNode {
                    node_type: "llm.prompt_writer".to_owned(),
                    title: "Prompt".to_owned(),
                    params: json!({ "style": "product" }),
                    pos: [220.0, 0.0],
                },
            ),
            (
                "video".to_owned(),
                GraphNode {
                    node_type: "video.text_to_video".to_owned(),
                    title: "Video".to_owned(),
                    params: json!({
                        "prompt": "clean product shot",
                        "duration_sec": 5,
                        "aspect_ratio": "9:16"
                    }),
                    pos: [440.0, 0.0],
                },
            ),
        ]),
        edges: vec![
            GraphEdge {
                from: ["input".to_owned(), "text".to_owned()],
                to: ["writer".to_owned(), "text".to_owned()],
                edge_type: "text".to_owned(),
            },
            GraphEdge {
                from: ["writer".to_owned(), "prompt".to_owned()],
                to: ["video".to_owned(), "prompt".to_owned()],
                edge_type: "text".to_owned(),
            },
        ],
    }
}

struct ManualRouteNoopWorkbenchAgent;

#[async_trait]
impl WorkbenchAgent for ManualRouteNoopWorkbenchAgent {
    async fn answer_chat(
        &self,
        _request: AgentSessionRequest,
    ) -> Result<ValidatedAgentReply, AgentError> {
        Err(AgentError::Runtime("noop agent".to_owned()))
    }

    async fn propose_graph_change(
        &self,
        _request: AgentSessionRequest,
    ) -> Result<ValidatedAgentProposal, AgentError> {
        Err(AgentError::Runtime("noop agent".to_owned()))
    }
}
