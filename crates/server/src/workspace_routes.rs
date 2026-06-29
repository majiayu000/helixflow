use std::path::PathBuf;

use axum::{Json, extract::State};
use helixflow_store::{NewVersion, VersionSource, WorkspaceRecord};
use serde::{Deserialize, Serialize};

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::{blank_graph, write_json_file};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateWorkspaceRequest {
    name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceSummary {
    id: String,
    name: String,
    version_id: String,
    updated_at: String,
}

pub(crate) async fn list_workspaces(
    State(state): State<AppState>,
) -> Result<Json<Vec<WorkspaceSummary>>, ApiError> {
    let workspaces = state.store.workspaces().await.map_err(ApiError::store)?;
    Ok(Json(workspaces.iter().map(workspace_summary).collect()))
}

pub(crate) async fn create_workspace(
    State(state): State<AppState>,
    Json(input): Json<CreateWorkspaceRequest>,
) -> Result<Json<WorkspaceSummary>, ApiError> {
    let name = input
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Helixflow Workspace");
    let workspace = state
        .store
        .create_workspace(name)
        .await
        .map_err(ApiError::store)?;
    let graph_path = initial_graph_path(&workspace.id);
    let graph = blank_graph();
    let graph_hash = write_json_file(
        &state.data_dir,
        &graph_path,
        &graph,
        "write workspace initial graph",
    )
    .await?;
    state
        .store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Initial graph",
            source: VersionSource::Manual,
            graph_path: graph_path.to_string_lossy().as_ref(),
            graph_hash: &graph_hash,
            parent_id: None,
        })
        .await
        .map_err(ApiError::store)?;
    let workspace = state
        .store
        .workspace(&workspace.id)
        .await
        .map_err(ApiError::store)?;

    Ok(Json(workspace_summary(&workspace)))
}

fn initial_graph_path(workspace_id: &str) -> PathBuf {
    PathBuf::from("workspaces")
        .join(workspace_id)
        .join("graphs")
        .join("initial.json")
}

fn workspace_summary(workspace: &WorkspaceRecord) -> WorkspaceSummary {
    WorkspaceSummary {
        id: workspace.id.clone(),
        name: workspace.name.clone(),
        version_id: workspace.cur_version_id.clone().unwrap_or_default(),
        updated_at: workspace.updated_at.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::extract::State;
    use helixflow_agent::{
        AgentError, AgentSessionRequest, ValidatedAgentProposal, ValidatedAgentReply,
    };
    use helixflow_run::EventBus;
    use helixflow_store::Store;

    use super::*;
    use crate::app_state::{AppState, WorkbenchAgent};

    #[tokio::test]
    async fn create_workspace_writes_initial_graph_and_version() {
        let (state, _dir) = test_state().await;

        let summary = create_workspace(
            State(state.clone()),
            Json(CreateWorkspaceRequest { name: None }),
        )
        .await
        .expect("create workspace")
        .0;

        assert_eq!(summary.name, "Helixflow Workspace");
        assert!(!summary.version_id.is_empty());
        let version = state
            .store
            .version(&summary.version_id)
            .await
            .expect("version");
        assert!(state.data_dir.join(version.graph_path).exists());
        assert!(version.graph_hash.starts_with("sha256:"));
    }

    #[tokio::test]
    async fn list_workspaces_returns_created_workspace() {
        let (state, _dir) = test_state().await;
        let created = create_workspace(
            State(state.clone()),
            Json(CreateWorkspaceRequest {
                name: Some("User workspace".to_owned()),
            }),
        )
        .await
        .expect("create workspace")
        .0;

        let workspaces = list_workspaces(State(state))
            .await
            .expect("list workspaces")
            .0;

        assert_eq!(workspaces.len(), 1);
        assert_eq!(workspaces[0].id, created.id);
        assert_eq!(workspaces[0].name, "User workspace");
    }

    async fn test_state() -> (AppState, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let state = AppState::with_store_agent(
            EventBus::new(16),
            store,
            data_dir.clone(),
            Arc::new(NoopWorkbenchAgent),
            data_dir.join("sessions"),
        );
        (state, dir)
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
