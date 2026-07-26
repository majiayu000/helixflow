use std::collections::BTreeSet;

use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_graph::{GraphError, GraphService, ProposalOp};
use helixflow_registry::NodeRegistry;
use helixflow_store::{NewVersion, StoreError, VersionSource};
use serde::Deserialize;
use serde_json::Value;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::version_file_consistency::{
    CandidateKind, VersionFileCandidate, VersionFileConsistencyError, read_version_graph,
};
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

#[cfg(test)]
#[derive(Clone)]
struct LayoutCommitHook {
    published: std::sync::Arc<tokio::sync::Barrier>,
    release: std::sync::Arc<tokio::sync::Barrier>,
}

#[cfg(test)]
impl LayoutCommitHook {
    async fn after_publish(&self) {
        self.published.wait().await;
        self.release.wait().await;
    }
}

pub(crate) async fn save_workspace_layout(
    path: Path<String>,
    state: State<AppState>,
    request: Json<SaveLayoutRequest>,
) -> Result<Json<Value>, ApiError> {
    #[cfg(test)]
    {
        save_workspace_layout_inner(path, state, request, None).await
    }
    #[cfg(not(test))]
    {
        save_workspace_layout_inner(path, state, request).await
    }
}

async fn save_workspace_layout_inner(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
    Json(request): Json<SaveLayoutRequest>,
    #[cfg(test)] commit_hook: Option<&LayoutCommitHook>,
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
    let current_graph = read_version_graph(&state.data_dir, &current)
        .await
        .map_err(candidate_error)?;
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
    let mut candidate =
        VersionFileCandidate::from_graph(&workspace_id, CandidateKind::Layout, &moved_graph)
            .map_err(candidate_error)?;
    let graph_path = candidate
        .relative_path_text()
        .map_err(candidate_error)?
        .to_owned();
    let graph_hash = candidate.graph_hash().to_owned();
    candidate
        .publish(&state.data_dir)
        .map_err(candidate_error)?;
    #[cfg(test)]
    if let Some(hook) = commit_hook {
        hook.after_publish().await;
    }
    let result = state
        .store
        .create_version_after_without_pending_proposal(
            NewVersion {
                workspace_id: &workspace_id,
                label: "Update layout",
                source: VersionSource::Manual,
                graph_path: &graph_path,
                graph_hash: &graph_hash,
                parent_id: Some(current_version_id),
                semantics_json: None,
            },
            current_version_id,
        )
        .await;
    match result {
        Ok(_) => candidate.mark_committed().map_err(candidate_error)?,
        Err(store_error) => {
            return Err(cleanup_layout_candidate(&state, &mut candidate, store_error).await);
        }
    }

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

async fn cleanup_layout_candidate(
    state: &AppState,
    candidate: &mut VersionFileCandidate,
    store_error: StoreError,
) -> ApiError {
    match candidate.cleanup_after_store_error(&state.store).await {
        Ok(_) => match store_error {
            StoreError::VersionConflict { .. } | StoreError::PendingProposalConflict { .. } => {
                ApiError::conflict("workspace changed while saving layout; refresh and retry")
            }
            error => ApiError::store(error),
        },
        Err(cleanup_error) => ApiError::server_error(format!(
            "layout commit failed and candidate cleanup was deferred: {cleanup_error}"
        )),
    }
}

fn candidate_error(error: VersionFileConsistencyError) -> ApiError {
    ApiError::server_error(error.to_string())
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
    use tokio::{
        sync::Barrier,
        time::{Duration, timeout},
    };

    use super::*;
    use crate::app_state::{AppState, WorkbenchAgent};
    use crate::graph_files::{graph_hash, read_graph_file};

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
        assert_eq!(
            read_version_graph(&state.data_dir, &version)
                .await
                .expect("verified layout graph"),
            read_graph_file(&state.data_dir, &version.graph_path)
                .await
                .expect("stored layout graph")
        );
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

    #[tokio::test]
    async fn concurrent_same_base_layout_has_one_winner_and_cleans_loser_candidate() {
        let (state, workspace_id, base_version_id, _dir) =
            state_with_layout_graph(layout_route_graph()).await;
        let hook = LayoutCommitHook {
            published: Arc::new(Barrier::new(3)),
            release: Arc::new(Barrier::new(3)),
        };
        let first = spawn_layout_save(
            state.clone(),
            hook.clone(),
            workspace_id.clone(),
            base_version_id.clone(),
            500.0,
        );
        let second = spawn_layout_save(
            state.clone(),
            hook.clone(),
            workspace_id.clone(),
            base_version_id.clone(),
            600.0,
        );
        wait_for_layout_phase(&hook.published, &first, &second, "publish").await;
        let published_entries = graph_entry_names(&state, &workspace_id).await;
        assert_eq!(published_entries.len(), 3, "entries: {published_entries:?}");
        assert_eq!(
            published_entries
                .iter()
                .filter(|name| *name == "base.json")
                .count(),
            1
        );
        assert_eq!(
            published_entries
                .iter()
                .filter(|name| name.starts_with("layout-") && name.ends_with(".json"))
                .count(),
            2,
            "entries: {published_entries:?}"
        );
        assert_no_candidate_temps(&published_entries);
        wait_for_layout_phase(&hook.release, &first, &second, "CAS release").await;
        let results = [
            first.await.expect("first layout task"),
            second.await.expect("second layout task"),
        ];

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter_map(|result| result.as_ref().err())
                .filter(|error| error.status == axum::http::StatusCode::CONFLICT)
                .count(),
            1,
            "results: {results:?}"
        );
        let workspace = state
            .store
            .workspace(&workspace_id)
            .await
            .expect("workspace after race");
        let winner_id = workspace.cur_version_id.expect("winner version id");
        let winner = state
            .store
            .version(&winner_id)
            .await
            .expect("winner version");
        let bytes = tokio::fs::read(state.data_dir.join(&winner.graph_path))
            .await
            .expect("winner bytes");
        let graph = read_version_graph(&state.data_dir, &winner)
            .await
            .expect("verified winner graph");
        assert_eq!(winner.parent_id.as_deref(), Some(base_version_id.as_str()));
        assert_eq!(winner.graph_hash, graph_hash(&bytes));
        assert!(matches!(graph.nodes["video"].pos[0], 500.0 | 600.0));
        assert_eq!(
            state
                .store
                .versions_for_workspace(&workspace_id)
                .await
                .expect("versions after race")
                .len(),
            2
        );
        let final_entries = graph_entry_names(&state, &workspace_id).await;
        assert_eq!(final_entries.len(), 2, "entries: {final_entries:?}");
        assert_no_candidate_temps(&final_entries);
    }

    fn spawn_layout_save(
        state: AppState,
        hook: LayoutCommitHook,
        workspace_id: String,
        base_version_id: String,
        x: f32,
    ) -> tokio::task::JoinHandle<Result<Json<Value>, ApiError>> {
        tokio::spawn(async move {
            save_workspace_layout_inner(
                Path(workspace_id),
                State(state),
                Json(SaveLayoutRequest {
                    base_version_id,
                    positions: vec![NodePositionUpdate {
                        id: "video".to_owned(),
                        x,
                        y: 90.0,
                    }],
                }),
                Some(&hook),
            )
            .await
        })
    }

    async fn wait_for_layout_phase(
        barrier: &Barrier,
        first: &tokio::task::JoinHandle<Result<Json<Value>, ApiError>>,
        second: &tokio::task::JoinHandle<Result<Json<Value>, ApiError>>,
        phase: &str,
    ) {
        if timeout(Duration::from_secs(5), barrier.wait())
            .await
            .is_err()
        {
            first.abort();
            second.abort();
            panic!("layout race timed out at {phase} barrier");
        }
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
        let graph_bytes = serde_json::to_vec_pretty(&graph).expect("graph json");
        tokio::fs::write(data_dir.join(&graph_path), &graph_bytes)
            .await
            .expect("write graph");
        let graph_path_string = graph_path.to_string_lossy().into_owned();
        let stored_graph_hash = graph_hash(&graph_bytes);
        let version = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Base graph",
                source: VersionSource::Manual,
                graph_path: &graph_path_string,
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
        let graph_bytes =
            serde_json::to_vec_pretty(&layout_route_graph()).expect("advanced graph json");
        tokio::fs::write(state.data_dir.join(&graph_path), &graph_bytes)
            .await
            .expect("write advanced graph");
        let graph_path_string = graph_path.to_string_lossy().into_owned();
        let stored_graph_hash = graph_hash(&graph_bytes);
        state
            .store
            .create_version_after(
                NewVersion {
                    workspace_id,
                    label: "Advanced graph",
                    source: VersionSource::Manual,
                    graph_path: &graph_path_string,
                    graph_hash: &stored_graph_hash,
                    parent_id: Some(base_version_id),
                    semantics_json: None,
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

    async fn graph_entry_names(state: &AppState, workspace_id: &str) -> Vec<String> {
        let mut entries = tokio::fs::read_dir(
            state
                .data_dir
                .join("workspaces")
                .join(workspace_id)
                .join("graphs"),
        )
        .await
        .expect("graph directory");
        let mut names = Vec::new();
        while let Some(entry) = entries.next_entry().await.expect("read graph entry") {
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
                        semantics: None,
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
                        semantics: None,
                    },
                ),
            ]),
            edges: vec![GraphEdge {
                from: ["input".to_owned(), "text".to_owned()],
                to: ["video".to_owned(), "prompt".to_owned()],
                edge_type: "text".to_owned(),
            }],
            catalog_revision: None,
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
