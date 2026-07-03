use std::collections::BTreeSet;
use std::path::PathBuf;

use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_graph::{GraphError, GraphService, ProposalOp};
use helixflow_registry::NodeRegistry;
use helixflow_store::{NewVersion, VersionSource};
use serde::Deserialize;
use serde_json::Value;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::{read_graph_file, write_json_file};
use crate::workspace_state::workspace_state_value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SaveLayoutRequest {
    base_version_id: String,
    positions: Vec<NodePositionUpdate>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodePositionUpdate {
    id: String,
    x: f32,
    y: f32,
}

pub(crate) async fn save_workspace_layout(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
    Json(request): Json<SaveLayoutRequest>,
) -> Result<Json<Value>, ApiError> {
    let workspace = state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    let current_version_id = workspace
        .cur_version_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("workspace has no current version"))?;
    if request.base_version_id.trim().is_empty() {
        return Err(ApiError::bad_request("baseVersionId is required"));
    }
    if request.base_version_id != current_version_id {
        return Err(ApiError::conflict(format!(
            "layout base `{}` is superseded by `{current_version_id}`",
            request.base_version_id
        )));
    }
    if state
        .store
        .latest_pending_proposal(&workspace_id)
        .await
        .map_err(ApiError::store)?
        .is_some()
    {
        return Err(ApiError::conflict(
            "workspace has a pending proposal; apply or dismiss it before saving layout",
        ));
    }
    if request.positions.is_empty() {
        return Err(ApiError::bad_request("layout positions cannot be empty"));
    }

    let current = state
        .store
        .version(current_version_id)
        .await
        .map_err(ApiError::store)?;
    let current_graph = read_graph_file(&state.data_dir, &current.graph_path).await?;
    let ops = layout_ops(&current_graph, &request.positions)?;
    if ops.is_empty() {
        return Err(ApiError::bad_request(
            "layout request has no position changes",
        ));
    }
    let graph_service = GraphService::new(NodeRegistry::builtin());
    let moved_graph = graph_service
        .apply_ops(&current_graph, &ops)
        .map_err(graph_layout_error)?;
    graph_service
        .validate_graph(&moved_graph)
        .map_err(graph_layout_error)?;
    let graph_path = layout_graph_path(&workspace_id, current_version_id);
    let graph_hash = write_json_file(
        &state.data_dir,
        &graph_path,
        &moved_graph,
        "write layout graph",
    )
    .await?;
    let graph_path_string = graph_path.to_string_lossy().into_owned();
    state
        .store
        .create_version_after(
            NewVersion {
                workspace_id: &workspace_id,
                label: "Update layout",
                source: VersionSource::Manual,
                graph_path: &graph_path_string,
                graph_hash: &graph_hash,
                parent_id: Some(current_version_id),
            },
            current_version_id,
        )
        .await
        .map_err(ApiError::store)?;

    Ok(Json(workspace_state_value(&state, &workspace_id).await?))
}

fn layout_ops(
    graph: &helixflow_graph::WorkflowGraph,
    positions: &[NodePositionUpdate],
) -> Result<Vec<ProposalOp>, ApiError> {
    let mut seen = BTreeSet::new();
    let mut ops = Vec::new();
    for position in positions {
        if position.id.trim().is_empty() {
            return Err(ApiError::bad_request("layout node id cannot be empty"));
        }
        if !seen.insert(position.id.as_str()) {
            return Err(ApiError::bad_request(format!(
                "duplicate layout position for node `{}`",
                position.id
            )));
        }
        if !position.x.is_finite() || !position.y.is_finite() {
            return Err(ApiError::bad_request(format!(
                "layout position for node `{}` must be finite",
                position.id
            )));
        }
        let node = graph.nodes.get(&position.id).ok_or_else(|| {
            ApiError::bad_request(format!("unknown layout node `{}`", position.id))
        })?;
        let next = [position.x, position.y];
        if node.pos != next {
            ops.push(ProposalOp::MoveNode {
                id: position.id.clone(),
                pos: next,
            });
        }
    }
    Ok(ops)
}

fn layout_graph_path(workspace_id: &str, base_version_id: &str) -> PathBuf {
    PathBuf::from("workspaces")
        .join(workspace_id)
        .join("graphs")
        .join(format!("layout-{base_version_id}.json"))
}

fn graph_layout_error(err: GraphError) -> ApiError {
    match err {
        GraphError::ProposalSuperseded { .. } => ApiError::conflict(err.to_string()),
        _ => ApiError::bad_request(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::extract::{Path, State};
    use helixflow_agent::{
        AgentError, AgentSessionRequest, ValidatedAgentProposal, ValidatedAgentReply,
    };
    use helixflow_graph::{GraphEdge, GraphNode, WorkflowGraph};
    use helixflow_run::EventBus;
    use helixflow_store::{NewProposal, NewVersion, Store, VersionSource};
    use serde_json::json;

    use super::*;
    use crate::app_state::{AppState, WorkbenchAgent};

    #[tokio::test]
    async fn save_workspace_layout_creates_manual_version_with_updated_positions() {
        let (state, workspace_id, base_version_id, _dir) =
            state_with_layout_graph(layout_route_graph()).await;

        let body = save_workspace_layout(
            Path(workspace_id.clone()),
            State(state.clone()),
            Json(SaveLayoutRequest {
                base_version_id: base_version_id.clone(),
                positions: vec![
                    NodePositionUpdate {
                        id: "input".to_owned(),
                        x: 12.0,
                        y: 34.0,
                    },
                    NodePositionUpdate {
                        id: "video".to_owned(),
                        x: 460.0,
                        y: 80.0,
                    },
                ],
            }),
        )
        .await
        .expect("save layout")
        .0;

        let next_version_id = body["workspace"]["versionId"].as_str().expect("version id");
        assert_ne!(next_version_id, base_version_id);
        assert_eq!(body["graph"]["nodes"][0]["position"]["x"], 12.0);
        assert_eq!(body["workflowGraph"]["nodes"]["video"]["pos"][1], 80.0);
        assert_eq!(
            body["workflowGraph"]["nodes"]["video"]["params"]["duration_sec"],
            5
        );
        assert_eq!(
            body["workflowGraph"]["edges"]
                .as_array()
                .expect("edges")
                .len(),
            1
        );

        let version = state.store.version(next_version_id).await.expect("version");
        assert_eq!(version.source, "manual");
        assert_eq!(version.label, "Update layout");
        assert_eq!(version.parent_id.as_deref(), Some(base_version_id.as_str()));
        assert!(state.data_dir.join(&version.graph_path).exists());
        assert_ne!(version.graph_hash, "sha256:base");
        assert!(
            body["history"]
                .as_array()
                .expect("history")
                .iter()
                .any(|item| item["id"] == next_version_id && item["label"] == "Update layout")
        );
    }

    #[tokio::test]
    async fn save_workspace_layout_rejects_stale_base_version() {
        let (state, workspace_id, base_version_id, _dir) =
            state_with_layout_graph(layout_route_graph()).await;
        advance_workspace_version(&state, &workspace_id, &base_version_id).await;

        let err = save_workspace_layout(
            Path(workspace_id.clone()),
            State(state.clone()),
            Json(SaveLayoutRequest {
                base_version_id: base_version_id.clone(),
                positions: vec![NodePositionUpdate {
                    id: "video".to_owned(),
                    x: 500.0,
                    y: 0.0,
                }],
            }),
        )
        .await
        .expect_err("stale layout save should conflict");

        assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
        let workspace = state
            .store
            .workspace(&workspace_id)
            .await
            .expect("workspace");
        assert_ne!(
            workspace.cur_version_id.as_deref(),
            Some(base_version_id.as_str())
        );
    }

    #[tokio::test]
    async fn save_workspace_layout_rejects_unknown_node_without_partial_write() {
        let (state, workspace_id, base_version_id, _dir) =
            state_with_layout_graph(layout_route_graph()).await;

        let err = save_workspace_layout(
            Path(workspace_id.clone()),
            State(state.clone()),
            Json(SaveLayoutRequest {
                base_version_id: base_version_id.clone(),
                positions: vec![
                    NodePositionUpdate {
                        id: "input".to_owned(),
                        x: 50.0,
                        y: 60.0,
                    },
                    NodePositionUpdate {
                        id: "missing".to_owned(),
                        x: 70.0,
                        y: 80.0,
                    },
                ],
            }),
        )
        .await
        .expect_err("unknown node should fail");

        assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
        let workspace = state
            .store
            .workspace(&workspace_id)
            .await
            .expect("workspace");
        assert_eq!(
            workspace.cur_version_id.as_deref(),
            Some(base_version_id.as_str())
        );
    }

    #[tokio::test]
    async fn save_workspace_layout_rejects_non_finite_position() {
        let (state, workspace_id, base_version_id, _dir) =
            state_with_layout_graph(layout_route_graph()).await;

        let err = save_workspace_layout(
            Path(workspace_id.clone()),
            State(state),
            Json(SaveLayoutRequest {
                base_version_id,
                positions: vec![NodePositionUpdate {
                    id: "input".to_owned(),
                    x: f32::NAN,
                    y: 60.0,
                }],
            }),
        )
        .await
        .expect_err("non-finite position should fail");

        assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn save_workspace_layout_rejects_pending_proposal_and_keeps_it_pending() {
        let (state, workspace_id, base_version_id, _dir) =
            state_with_layout_graph(layout_route_graph()).await;
        let proposal_id = create_pending_proposal(&state, &workspace_id, &base_version_id).await;

        let err = save_workspace_layout(
            Path(workspace_id.clone()),
            State(state.clone()),
            Json(SaveLayoutRequest {
                base_version_id: base_version_id.clone(),
                positions: vec![NodePositionUpdate {
                    id: "video".to_owned(),
                    x: 500.0,
                    y: 90.0,
                }],
            }),
        )
        .await
        .expect_err("pending proposal should block layout save");

        assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
        let proposal = state.store.proposal(&proposal_id).await.expect("proposal");
        assert_eq!(proposal.state, "pending");
        let workspace = state
            .store
            .workspace(&workspace_id)
            .await
            .expect("workspace");
        assert_eq!(
            workspace.cur_version_id.as_deref(),
            Some(base_version_id.as_str())
        );
    }

    async fn state_with_layout_graph(
        graph: WorkflowGraph,
    ) -> (AppState, String, String, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store
            .create_workspace("Layout route workspace")
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
            Arc::new(RejectingLayoutAgent),
            data_dir.join("sessions"),
        );
        (state, workspace.id, version.id, dir)
    }

    async fn advance_workspace_version(
        state: &AppState,
        workspace_id: &str,
        base_version_id: &str,
    ) {
        let graph_path = PathBuf::from("workspaces")
            .join(workspace_id)
            .join("graphs")
            .join("advanced.json");
        tokio::fs::write(
            state.data_dir.join(&graph_path),
            serde_json::to_vec_pretty(&layout_route_graph()).expect("advanced graph json"),
        )
        .await
        .expect("write advanced graph");
        let graph_path_string = graph_path.to_string_lossy().into_owned();
        state
            .store
            .create_version_after(
                NewVersion {
                    workspace_id,
                    label: "Advanced graph",
                    source: VersionSource::Manual,
                    graph_path: &graph_path_string,
                    graph_hash: "sha256:advanced",
                    parent_id: Some(base_version_id),
                },
                base_version_id,
            )
            .await
            .expect("advance version");
    }

    async fn create_pending_proposal(
        state: &AppState,
        workspace_id: &str,
        base_version_id: &str,
    ) -> String {
        let proposal_dir = PathBuf::from("workspaces")
            .join(workspace_id)
            .join("proposals")
            .join("layout-test");
        tokio::fs::create_dir_all(state.data_dir.join(&proposal_dir))
            .await
            .expect("proposal dir");
        let ops_path = proposal_dir.join("ops.json");
        let preview_path = proposal_dir.join("preview.json");
        tokio::fs::write(state.data_dir.join(&ops_path), b"[]")
            .await
            .expect("write ops");
        tokio::fs::write(
            state.data_dir.join(&preview_path),
            serde_json::to_vec_pretty(&layout_route_graph()).expect("preview graph json"),
        )
        .await
        .expect("write preview");
        let ops_path_string = ops_path.to_string_lossy().into_owned();
        let preview_path_string = preview_path.to_string_lossy().into_owned();
        state
            .store
            .create_proposal(NewProposal {
                workspace_id,
                base_version_id,
                kind: "modify",
                title: "Pending layout blocker",
                summary: "Pending proposal should block layout save.",
                ops_path: &ops_path_string,
                preview_graph_path: Some(&preview_path_string),
                message_id: None,
            })
            .await
            .expect("create proposal")
            .id
    }

    fn layout_route_graph() -> WorkflowGraph {
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
                        size: None,
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
                        size: None,
                    },
                ),
            ]),
            edges: vec![GraphEdge {
                from: ["input".to_owned(), "text".to_owned()],
                to: ["video".to_owned(), "prompt".to_owned()],
                edge_type: "text".to_owned(),
            }],
        }
    }

    struct RejectingLayoutAgent;

    #[async_trait]
    impl WorkbenchAgent for RejectingLayoutAgent {
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
}
