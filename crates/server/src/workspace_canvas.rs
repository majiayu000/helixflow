use axum::{
    Json,
    extract::{Path as AxumPath, State},
};
use helixflow_graph::WorkflowGraph;
use serde_json::{Value, json};

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::canvas_collaboration::canvas_comment_snapshot;
use crate::graph_files::{blank_graph, read_graph_file};

pub(crate) async fn workspace_canvas(
    AxumPath(workspace_id): AxumPath<String>,
    State(state): State<AppState>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(workspace_canvas_value(&state, &workspace_id).await?))
}

pub(crate) async fn workspace_canvas_value(
    state: &AppState,
    workspace_id: &str,
) -> Result<Value, ApiError> {
    let workspace = state
        .store
        .workspace(workspace_id)
        .await
        .map_err(ApiError::store)?;
    let (version_id, seq, graph) = match workspace.cur_version_id.as_deref() {
        Some(version_id) => {
            let version = state
                .store
                .version(version_id)
                .await
                .map_err(ApiError::store)?;
            let graph = read_graph_file(&state.data_dir, &version.graph_path).await?;
            (version.id, version.idx, graph)
        }
        None => (String::new(), 0, blank_graph()),
    };
    let (comment_seq, comments) =
        canvas_comment_snapshot(&state.store, &state.data_dir, &workspace.id).await?;

    Ok(canvas_document_payload(
        &workspace.id,
        &version_id,
        seq.max(comment_seq),
        &graph,
        comments,
    ))
}

fn canvas_document_payload(
    workspace_id: &str,
    version_id: &str,
    seq: i64,
    graph: &WorkflowGraph,
    comments: Vec<crate::canvas_collaboration::CanvasComment>,
) -> Value {
    json!({
        "schemaVersion": 1,
        "workspaceId": workspace_id,
        "versionId": version_id,
        "seq": seq,
        "nodes": graph.nodes.iter().map(|(id, node)| {
            json!({
                "id": id,
                "nodeType": node.node_type,
                "title": node.title,
                "position": {
                    "x": node.pos[0],
                    "y": node.pos[1],
                },
                "size": node.size.map(|size| json!({
                    "width": size[0],
                    "height": size[1],
                })),
                "params": node.params,
                "runtime": Value::Null,
                "metadata": {
                    "source": "workflow_graph_compat",
                },
            })
        }).collect::<Vec<_>>(),
        "edges": graph.edges.iter().enumerate().map(|(idx, edge)| {
            json!({
                "id": format!(
                    "edge_{}_{}_{}_{}_{}",
                    edge.from[0], edge.from[1], edge.to[0], edge.to[1], idx
                ),
                "from": {
                    "nodeId": edge.from[0],
                    "port": edge.from[1],
                },
                "to": {
                    "nodeId": edge.to[0],
                    "port": edge.to[1],
                },
                "kind": edge.edge_type,
            })
        }).collect::<Vec<_>>(),
        "comments": comments,
        "runtime": {},
        "metadata": {
            "source": "workflow_graph_compat",
            "graphSchemaVersion": graph.schema_version,
        },
    })
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use axum::extract::State;
    use helixflow_graph::{GraphEdge, GraphNode};
    use helixflow_run::EventBus;
    use helixflow_store::{NewVersion, Store, VersionSource};
    use serde_json::json;

    use super::*;
    use crate::app_state::AppState;
    use crate::graph_files::write_json_file;
    use crate::test_support::FailingWorkbenchAgent;

    #[tokio::test]
    async fn workspace_canvas_projects_current_graph_as_canvas_document() {
        let (state, workspace_id, _dir) = state_with_workspace().await;

        let body = workspace_canvas(AxumPath(workspace_id.clone()), State(state))
            .await
            .expect("workspace canvas")
            .0;

        assert_eq!(body["workspaceId"], workspace_id);
        assert_eq!(body["schemaVersion"], 1);
        assert_eq!(body["seq"], 1);
        assert_eq!(body["nodes"][0]["id"], "text");
        assert_eq!(body["nodes"][0]["nodeType"], "input.text");
        assert_eq!(body["nodes"][0]["position"]["x"], 10.0);
        assert_eq!(body["nodes"][0]["size"]["width"], 260.0);
        assert_eq!(body["nodes"][0]["size"]["height"], 180.0);
        assert_eq!(body["edges"][0]["from"]["nodeId"], "text");
        assert_eq!(body["metadata"]["source"], "workflow_graph_compat");
    }

    #[tokio::test]
    async fn workspace_canvas_replays_comment_snapshot_when_seq_is_ahead_of_graph_version() {
        let (state, workspace_id, _dir) = state_with_workspace().await;
        write_json_file(
            &state.data_dir,
            &comments_path(&workspace_id),
            &comment_store_json(7),
            "write comments",
        )
        .await
        .expect("write comments");

        let body = workspace_canvas(AxumPath(workspace_id.clone()), State(state))
            .await
            .expect("workspace canvas")
            .0;

        assert_eq!(body["seq"], 7);
        assert_eq!(body["comments"][0]["id"], "comment_replay");
        assert_eq!(body["comments"][0]["target"]["nodeId"], "text");
    }

    #[tokio::test]
    async fn workspace_canvas_rejects_snapshot_data_ahead_of_seq() {
        let (state, workspace_id, _dir) = state_with_workspace().await;
        write_json_file(
            &state.data_dir,
            &comments_path(&workspace_id),
            &comment_store_json(0),
            "write stale comments",
        )
        .await
        .expect("write comments");

        let err = workspace_canvas(AxumPath(workspace_id), State(state))
            .await
            .expect_err("snapshot ahead of seq should fail");

        assert!(err.message.contains("snapshot is ahead of seq"));
    }

    #[tokio::test]
    async fn workspace_canvas_returns_blank_canvas_without_current_version() {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let store = open_canvas_store(dir.path()).await;
        let workspace = store
            .create_workspace("Blank")
            .await
            .expect("create workspace");
        let state = canvas_test_state(store, data_dir);

        let body = workspace_canvas(AxumPath(workspace.id), State(state))
            .await
            .expect("workspace canvas")
            .0;

        assert_eq!(body["nodes"].as_array().expect("nodes").len(), 0);
        assert_eq!(body["edges"].as_array().expect("edges").len(), 0);
    }

    #[tokio::test]
    async fn workspace_canvas_uses_app_state_store_for_comment_snapshot() {
        let store_dir = tempfile::tempdir().expect("store dir");
        let data_dir = tempfile::tempdir().expect("data dir");
        let store = open_canvas_store(store_dir.path()).await;
        let workspace = store
            .create_workspace("Store-backed comments")
            .await
            .expect("create workspace");
        let state = canvas_test_state(store, data_dir.path().to_path_buf());

        let body = workspace_canvas(AxumPath(workspace.id.clone()), State(state.clone()))
            .await
            .expect("workspace canvas from AppState store")
            .0;

        assert_eq!(body["workspaceId"], workspace.id);
        assert_eq!(body["seq"], 0);
        assert_eq!(body["comments"], json!([]));
        assert!(
            state
                .store
                .canvas_comment_state(&workspace.id)
                .await
                .expect("read comment state")
                .is_some()
        );
        assert_eq!(
            std::fs::read_dir(data_dir.path())
                .expect("read isolated data dir")
                .count(),
            0,
            "canvas read must not open a new Store in data_dir"
        );
    }

    async fn state_with_workspace() -> (AppState, String, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let store = open_canvas_store(dir.path()).await;
        let workspace = store
            .create_workspace("Canvas workspace")
            .await
            .expect("create workspace");
        let graph_path = std::path::PathBuf::from("graphs/current.json");
        let graph_hash = write_json_file(&data_dir, &graph_path, &sample_graph(), "write graph")
            .await
            .expect("write graph");
        store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Current graph",
                source: VersionSource::Manual,
                graph_path: graph_path.to_string_lossy().as_ref(),
                graph_hash: &graph_hash,
                parent_id: None,
            })
            .await
            .expect("create version");
        let state = canvas_test_state(store, data_dir);
        (state, workspace.id, dir)
    }

    async fn open_canvas_store(path: &std::path::Path) -> Store {
        let database_url = format!("sqlite://{}", path.join("helixflow.sqlite").display());
        Store::open(&database_url).await.expect("open store")
    }

    fn canvas_test_state(store: Store, data_dir: std::path::PathBuf) -> AppState {
        AppState::with_store_agent(
            EventBus::new(16),
            store,
            data_dir.clone(),
            Arc::new(FailingWorkbenchAgent),
            data_dir.join("sessions"),
        )
    }

    fn comments_path(workspace_id: &str) -> std::path::PathBuf {
        std::path::PathBuf::from("canvas")
            .join(workspace_id)
            .join("comments.json")
    }

    fn comment_store_json(seq: i64) -> serde_json::Value {
        json!({
            "schemaVersion": 1,
            "seq": seq,
            "comments": [
                {
                    "id": "comment_replay",
                    "target": { "kind": "node", "nodeId": "text" },
                    "body": "Reload proof",
                    "author": { "actorId": "reviewer", "displayName": "Reviewer" },
                    "status": "open",
                    "createdAt": "unix:1",
                    "updatedAt": "unix:1"
                }
            ]
        })
    }

    fn sample_graph() -> WorkflowGraph {
        WorkflowGraph {
            schema_version: 1,
            nodes: BTreeMap::from([
                (
                    "text".to_owned(),
                    GraphNode {
                        node_type: "input.text".to_owned(),
                        title: "Text".to_owned(),
                        params: json!({ "text": "hello" }),
                        pos: [10.0, 20.0],
                        size: Some([260.0, 180.0]),
                    },
                ),
                (
                    "video".to_owned(),
                    GraphNode {
                        node_type: "video.text_to_video".to_owned(),
                        title: "Video".to_owned(),
                        params: json!({ "prompt": "hello", "duration_sec": 4 }),
                        pos: [300.0, 20.0],
                        size: None,
                    },
                ),
            ]),
            edges: vec![GraphEdge {
                from: ["text".to_owned(), "text".to_owned()],
                to: ["video".to_owned(), "prompt".to_owned()],
                edge_type: "text".to_owned(),
            }],
        }
    }
}
