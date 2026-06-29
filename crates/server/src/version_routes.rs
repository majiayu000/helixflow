use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_graph::WorkflowGraph;
use serde_json::Value;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::read_graph_file;

pub(crate) async fn export_workflow_version(
    Path(version_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<WorkflowGraph>, ApiError> {
    let version = match state.store.version(&version_id).await {
        Ok(version) => version,
        Err(err) if err.is_not_found() => return Err(ApiError::not_found("version was not found")),
        Err(err) => return Err(ApiError::store(err)),
    };
    let graph = read_graph_file(&state.data_dir, &version.graph_path).await?;
    ensure_exportable_graph(&graph)?;
    Ok(Json(graph))
}

fn ensure_exportable_graph(graph: &WorkflowGraph) -> Result<(), ApiError> {
    let value = serde_json::to_value(graph).map_err(|err| {
        ApiError::server_error(format!("encode workflow graph for export: {err}"))
    })?;
    ensure_exportable_json(&value, "$")
}

fn ensure_exportable_json(value: &Value, path: &str) -> Result<(), ApiError> {
    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                let nested_path = format!("{path}.{key}");
                if is_non_exportable_key(key) {
                    return Err(ApiError::bad_request(format!(
                        "workflow graph contains non-exportable field `{nested_path}`"
                    )));
                }
                ensure_exportable_json(nested, &nested_path)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for (index, nested) in items.iter().enumerate() {
                ensure_exportable_json(nested, &format!("{path}[{index}]"))?;
            }
            Ok(())
        }
        Value::String(text) if looks_like_local_absolute_path(text) => Err(ApiError::bad_request(
            format!("workflow graph contains non-exportable local path at `{path}`"),
        )),
        _ => Ok(()),
    }
}

fn is_non_exportable_key(key: &str) -> bool {
    let normalized = key
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();

    normalized.contains("secret")
        || normalized.contains("password")
        || normalized.contains("credential")
        || normalized.contains("private_key")
        || normalized.contains("api_key")
        || normalized.contains("apikey")
        || normalized.contains("access_token")
        || normalized.contains("refresh_token")
        || normalized.contains("authorization")
        || normalized.contains("auth_header")
        || normalized.contains("request_header")
        || normalized == "headers"
        || normalized == "header"
        || normalized.contains("cookie")
        || normalized == "run_id"
        || normalized == "session_id"
        || normalized == "trace_id"
        || normalized == "provider_request_id"
        || normalized.contains("runtime_metadata")
}

fn looks_like_local_absolute_path(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("/Users/")
        || trimmed.starts_with("/home/")
        || trimmed.starts_with("/tmp/")
        || trimmed.starts_with("/private/")
        || trimmed.starts_with("/Volumes/")
        || trimmed.starts_with("~/")
        || trimmed.as_bytes().get(1) == Some(&b':')
            && matches!(trimmed.as_bytes().get(2), Some(b'\\' | b'/'))
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
    async fn export_workflow_version_returns_current_graph_when_proposal_is_pending() {
        let (state, version_id, _dir) = state_with_current_graph_and_pending_proposal().await;

        let body = export_workflow_version(Path(version_id), State(state))
            .await
            .expect("export version")
            .0;

        assert_eq!(
            body.nodes["video"].params["duration_sec"],
            json!(5),
            "export must return current version graph, not pending proposal preview"
        );
        let encoded = serde_json::to_string(&body).expect("export json");
        assert!(!encoded.contains("api_key"));
        assert!(!encoded.contains("/Users/"));
    }

    #[tokio::test]
    async fn export_workflow_version_rejects_unsafe_graph_params() {
        let (state, version_id, _dir) = state_with_graph(unsafe_graph()).await;

        let err = export_workflow_version(Path(version_id), State(state))
            .await
            .expect_err("unsafe export should fail");

        assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
        assert!(err.message.contains("non-exportable field"));
        assert!(!err.message.contains("sk_live"));
    }

    async fn state_with_current_graph_and_pending_proposal() -> (AppState, String, tempfile::TempDir)
    {
        let (state, version_id, dir) = state_with_graph(current_graph()).await;
        let workspace_id = state
            .store
            .version(&version_id)
            .await
            .expect("version")
            .workspace_id;
        let proposal_dir = PathBuf::from("workspaces")
            .join(&workspace_id)
            .join("proposals")
            .join("export-test");
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
            serde_json::to_vec_pretty(&preview_graph()).expect("preview json"),
        )
        .await
        .expect("write preview");
        let ops_path_string = ops_path.to_string_lossy().into_owned();
        let preview_path_string = preview_path.to_string_lossy().into_owned();
        state
            .store
            .create_proposal(NewProposal {
                workspace_id: &workspace_id,
                base_version_id: &version_id,
                kind: "modify",
                title: "Shorter clip",
                summary: "Set duration to four seconds.",
                ops_path: &ops_path_string,
                preview_graph_path: Some(&preview_path_string),
                message_id: None,
            })
            .await
            .expect("create proposal");
        (state, version_id, dir)
    }

    async fn state_with_graph(graph: WorkflowGraph) -> (AppState, String, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store
            .create_workspace("Export route workspace")
            .await
            .expect("create workspace");
        let graph_path = PathBuf::from("workspaces")
            .join(&workspace.id)
            .join("graphs")
            .join("current.json");
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
                label: "Current graph",
                source: VersionSource::Manual,
                graph_path: &graph_path_string,
                graph_hash: "sha256:current",
                parent_id: None,
            })
            .await
            .expect("create version");
        let state = AppState::with_store_agent(
            EventBus::new(16),
            store,
            data_dir.clone(),
            Arc::new(NoopWorkbenchAgent),
            data_dir.join("sessions"),
        );
        (state, version.id, dir)
    }

    fn current_graph() -> WorkflowGraph {
        workflow_graph(json!({ "prompt": "make a product clip", "duration_sec": 5 }))
    }

    fn preview_graph() -> WorkflowGraph {
        workflow_graph(json!({ "prompt": "make a product clip", "duration_sec": 4 }))
    }

    fn unsafe_graph() -> WorkflowGraph {
        workflow_graph(json!({ "api_key": "sk_live_should_not_leak" }))
    }

    fn workflow_graph(params: serde_json::Value) -> WorkflowGraph {
        WorkflowGraph {
            schema_version: 1,
            nodes: BTreeMap::from([
                (
                    "input".to_owned(),
                    GraphNode {
                        node_type: "input.text".to_owned(),
                        title: "Text".to_owned(),
                        params: json!({ "text": "launch teaser" }),
                        pos: [0.0, 0.0],
                    },
                ),
                (
                    "video".to_owned(),
                    GraphNode {
                        node_type: "video.mock.text_to_video".to_owned(),
                        title: "Video render".to_owned(),
                        params,
                        pos: [240.0, 0.0],
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

    struct NoopWorkbenchAgent;

    #[async_trait]
    impl WorkbenchAgent for NoopWorkbenchAgent {
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
