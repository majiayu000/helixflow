use helixflow_agent::{CanvasGateState, CanvasOpsContext, CanvasSelection, TurnMode};
use helixflow_graph::WorkflowGraph;
use serde::Deserialize;

use crate::api_error::ApiError;
use crate::app_state::AppState;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkspaceCanvasContext {
    pub(crate) selection: WorkspaceCanvasSelection,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkspaceCanvasSelection {
    pub(crate) node_ids: Vec<String>,
}

pub(crate) async fn prepare_agent_canvas_context(
    state: &AppState,
    workspace_id: &str,
    turn_mode: TurnMode,
    base_version_id: &str,
    graph: &WorkflowGraph,
    input: Option<WorkspaceCanvasContext>,
) -> Result<Option<CanvasOpsContext>, ApiError> {
    let has_pending_proposal = state
        .store
        .latest_pending_proposal(workspace_id)
        .await
        .map_err(ApiError::store)?
        .is_some();
    if turn_mode_uses_proposal(turn_mode) && has_pending_proposal {
        return Err(ApiError::bad_request(
            "workspace already has a pending proposal; apply or dismiss it first",
        ));
    }
    if !turn_mode.uses_canvas_context() {
        return Ok(None);
    }

    Ok(Some(CanvasOpsContext::from_graph(
        workspace_id,
        base_version_id,
        graph,
        CanvasSelection {
            node_ids: input
                .map(|context| context.selection.node_ids)
                .unwrap_or_default(),
        },
        CanvasGateState {
            pending_proposal: has_pending_proposal,
            pending_confirmation: workspace_has_pending_confirmation(state, workspace_id).await?,
        },
    )))
}

fn turn_mode_uses_proposal(turn_mode: TurnMode) -> bool {
    matches!(
        turn_mode,
        TurnMode::CreateWorkflow | TurnMode::ModifyWorkflow | TurnMode::DebugWorkflow
    )
}

async fn workspace_has_pending_confirmation(
    state: &AppState,
    workspace_id: &str,
) -> Result<bool, ApiError> {
    Ok(state
        .store
        .latest_workspace_run(workspace_id)
        .await
        .map_err(ApiError::store)?
        .is_some_and(|run| run.status == "waiting_confirmation"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use async_trait::async_trait;
    use helixflow_agent::{AgentError, AgentSessionRequest, ValidatedAgentReply};
    use helixflow_graph::{GraphNode, WorkflowGraph};
    use helixflow_run::EventBus;
    use helixflow_store::{NewProposal, NewVersion, Store, VersionSource};
    use serde_json::json;

    use super::*;
    use crate::app_state::{AppState, WorkbenchAgent};

    #[tokio::test]
    async fn rejects_agent_proposal_when_pending_proposal_exists() {
        let (state, workspace_id, version_id, _dir) = state_with_workspace().await;
        state
            .store
            .create_proposal(NewProposal {
                workspace_id: &workspace_id,
                base_version_id: &version_id,
                kind: "modify",
                title: "Pending proposal",
                summary: "Existing proposal.",
                ops_path: "proposals/existing/ops.json",
                preview_graph_path: None,
                message_id: None,
            })
            .await
            .expect("create pending proposal");

        let err = prepare_agent_canvas_context(
            &state,
            &workspace_id,
            TurnMode::ModifyWorkflow,
            &version_id,
            &sample_graph(),
            None,
        )
        .await
        .expect_err("pending proposal should block proposal mode");

        assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
        assert!(err.message.contains("already has a pending proposal"));
        assert_eq!(
            state
                .store
                .workspace_proposals(&workspace_id)
                .await
                .expect("proposals")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn filters_canvas_selection_against_current_graph() {
        let (state, workspace_id, version_id, _dir) = state_with_workspace().await;
        let context = prepare_agent_canvas_context(
            &state,
            &workspace_id,
            TurnMode::RunRequest,
            &version_id,
            &sample_graph(),
            Some(WorkspaceCanvasContext {
                selection: WorkspaceCanvasSelection {
                    node_ids: vec![
                        "node_a".to_owned(),
                        "missing".to_owned(),
                        "node_a".to_owned(),
                    ],
                },
            }),
        )
        .await
        .expect("canvas context")
        .expect("graph context");

        assert_eq!(context.selection.node_ids, vec!["node_a"]);
        assert!(!context.gates.pending_proposal);
    }

    async fn state_with_workspace() -> (AppState, String, String, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store
            .create_workspace("Canvas message workspace")
            .await
            .expect("create workspace");
        let graph_path = PathBuf::from("graphs/canvas-message.json");
        let graph_path_string = graph_path.to_string_lossy().into_owned();
        let version = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Message graph",
                source: VersionSource::Manual,
                graph_path: &graph_path_string,
                graph_hash: "sha256:message",
                parent_id: None,
                semantics_json: None,
            })
            .await
            .expect("create version");
        let state = AppState::with_store_agent(
            EventBus::new(16),
            store,
            data_dir.clone(),
            Arc::new(CanvasNoopWorkbenchAgent),
            data_dir.join("sessions"),
        );
        (state, workspace.id, version.id, dir)
    }

    fn sample_graph() -> WorkflowGraph {
        WorkflowGraph {
            schema_version: 1,
            nodes: BTreeMap::from([(
                "node_a".to_owned(),
                GraphNode {
                    node_type: "input.text".to_owned(),
                    title: "Text".to_owned(),
                    params: json!({ "text": "hello" }),
                    pos: [0.0, 0.0],
                    size: None,
                    semantics: None,
                },
            )]),
            edges: Vec::new(),
            catalog_revision: None,
        }
    }

    struct CanvasNoopWorkbenchAgent;

    #[async_trait]
    impl WorkbenchAgent for CanvasNoopWorkbenchAgent {
        async fn answer_chat(
            &self,
            _request: AgentSessionRequest,
        ) -> Result<ValidatedAgentReply, AgentError> {
            Err(AgentError::Runtime("noop agent".to_owned()))
        }
    }
}
