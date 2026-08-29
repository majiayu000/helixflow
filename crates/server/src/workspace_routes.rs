use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_store::{
    ConversationRecord, CreateWorkspaceWithInitialVersion, InitializedWorkspace,
    ReservedWorkspaceIdentity, StoreError, VersionSource, WorkspaceRecord, WorkspaceSummaryRecord,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::blank_graph;
use crate::version_file_consistency::{
    CandidateKind, VersionFileCandidate, VersionFileConsistencyError,
};
use crate::workspace_state::workspace_state_value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateWorkspaceRequest {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetWorkspaceProviderRequest {
    provider_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateConversationRequest {
    title: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConversationPayload {
    id: String,
    workspace_id: String,
    title: String,
    codex_thread_id: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceSummary {
    id: String,
    name: String,
    version_id: String,
    updated_at: String,
    message_count: i64,
    first_message: Option<String>,
}

pub(crate) async fn list_workspaces(
    State(state): State<AppState>,
) -> Result<Json<Vec<WorkspaceSummary>>, ApiError> {
    let summaries = state
        .store
        .workspace_summaries()
        .await
        .map_err(ApiError::store)?;
    Ok(Json(
        summaries.into_iter().map(WorkspaceSummary::from).collect(),
    ))
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
    let identity = state.store.reserve_workspace_identity();
    let initialized = initialize_workspace(&state, identity, name).await?;

    Ok(Json(
        workspace_summary(&state, &initialized.workspace).await?,
    ))
}

pub(crate) async fn create_workspace_conversation(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
    Json(input): Json<CreateConversationRequest>,
) -> Result<Json<ConversationPayload>, ApiError> {
    let title = input
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("新对话");
    let conversation = state
        .store
        .create_conversation(&workspace_id, title)
        .await
        .map_err(ApiError::store)?;
    Ok(Json(ConversationPayload::from(conversation)))
}

impl From<ConversationRecord> for ConversationPayload {
    fn from(record: ConversationRecord) -> Self {
        Self {
            id: record.id,
            workspace_id: record.workspace_id,
            title: record.title,
            codex_thread_id: record.codex_thread_id,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

impl From<WorkspaceSummaryRecord> for WorkspaceSummary {
    fn from(record: WorkspaceSummaryRecord) -> Self {
        Self {
            id: record.id,
            name: record.name,
            version_id: record.version_id,
            updated_at: record.updated_at,
            message_count: record.message_count,
            first_message: record.first_message,
        }
    }
}

pub(crate) async fn set_workspace_provider(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
    Json(input): Json<SetWorkspaceProviderRequest>,
) -> Result<Json<Value>, ApiError> {
    let provider_id = input.provider_id.trim();
    if provider_id.is_empty() {
        return Err(ApiError::bad_request("providerId must not be empty"));
    }
    if !state.provider_registry.contains_provider(provider_id) {
        return Err(ApiError::bad_request(format!(
            "runtime provider `{provider_id}` is not registered"
        )));
    }
    if !state.provider_registry.provider_enabled(provider_id) {
        return Err(ApiError::conflict(format!(
            "runtime provider `{provider_id}` is registered but unavailable"
        )));
    }
    state
        .store
        .set_workspace_runtime_provider(&workspace_id, Some(provider_id))
        .await
        .map_err(ApiError::store)?;
    workspace_state_value(&state, &workspace_id).await.map(Json)
}

async fn initialize_workspace(
    state: &AppState,
    identity: ReservedWorkspaceIdentity,
    name: &str,
) -> Result<InitializedWorkspace, ApiError> {
    let mut candidate = VersionFileCandidate::from_graph(
        &identity.workspace_id,
        CandidateKind::Initial,
        &blank_graph(),
    )
    .map_err(candidate_error)?;
    let graph_path = candidate
        .relative_path_text()
        .map_err(candidate_error)?
        .to_owned();
    let graph_hash = candidate.graph_hash().to_owned();
    candidate
        .publish(&state.data_dir)
        .map_err(candidate_error)?;
    commit_initial_candidate(
        state,
        &mut candidate,
        state
            .store
            .create_workspace_with_initial_version(CreateWorkspaceWithInitialVersion {
                identity,
                name,
                version_label: "Initial graph",
                source: VersionSource::Manual,
                graph_path: &graph_path,
                graph_hash: &graph_hash,
            }),
    )
    .await
}

async fn commit_initial_candidate(
    state: &AppState,
    candidate: &mut VersionFileCandidate,
    commit: impl std::future::Future<Output = Result<InitializedWorkspace, StoreError>>,
) -> Result<InitializedWorkspace, ApiError> {
    match commit.await {
        Ok(initialized) => {
            candidate.mark_committed().map_err(candidate_error)?;
            Ok(initialized)
        }
        Err(store_error) => Err(cleanup_initial_candidate(state, candidate, store_error).await),
    }
}

async fn cleanup_initial_candidate(
    state: &AppState,
    candidate: &mut VersionFileCandidate,
    store_error: StoreError,
) -> ApiError {
    match candidate.cleanup_after_store_error(&state.store).await {
        Ok(_) => ApiError::store(store_error),
        Err(cleanup_error) => ApiError::server_error(format!(
            "initial workspace commit failed and candidate cleanup was deferred: {cleanup_error}"
        )),
    }
}

fn candidate_error(error: VersionFileConsistencyError) -> ApiError {
    ApiError::server_error(error.to_string())
}

async fn workspace_summary(
    state: &AppState,
    workspace: &WorkspaceRecord,
) -> Result<WorkspaceSummary, ApiError> {
    let (message_count, first_message) = state
        .store
        .workspace_message_stats(&workspace.id)
        .await
        .map_err(ApiError::store)?;
    Ok(WorkspaceSummary {
        id: workspace.id.clone(),
        name: workspace.name.clone(),
        version_id: workspace.cur_version_id.clone().unwrap_or_default(),
        updated_at: workspace.updated_at.clone(),
        message_count,
        first_message,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::extract::State;
    use helixflow_agent::{
        AgentError, AgentSessionRequest, ValidatedAgentProposal, ValidatedAgentReply,
    };
    use helixflow_gateway::{ProviderRegistry, RuntimeProvider};
    use helixflow_run::EventBus;
    use helixflow_store::Store;

    use super::*;
    use crate::app_state::{AppState, WorkbenchAgent};
    use crate::graph_files::graph_hash;
    use crate::version_file_consistency::read_version_graph;

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
        let graph_path = state.data_dir.join(&version.graph_path);
        let bytes = tokio::fs::read(&graph_path).await.expect("initial bytes");
        assert!(graph_path.exists());
        assert!(version.graph_path.contains("/initial-"));
        assert_eq!(version.graph_hash, graph_hash(&bytes));
        assert_eq!(
            read_version_graph(&state.data_dir, &version)
                .await
                .expect("verified initial graph"),
            blank_graph()
        );
    }

    #[tokio::test]
    async fn create_workspace_sql_failure_removes_unreferenced_initial_candidate() {
        let (state, _dir) = test_state().await;
        let existing = state
            .store
            .create_workspace("Existing")
            .await
            .expect("create collision workspace");
        let mut identity = state.store.reserve_workspace_identity();
        identity.workspace_id = existing.id.clone();

        let error = initialize_workspace(&state, identity.clone(), "Rejected")
            .await
            .expect_err("duplicate workspace identity must fail");

        assert_eq!(error.status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            state
                .store
                .version(&identity.initial_version_id)
                .await
                .is_err()
        );
        assert!(
            state
                .store
                .versions_for_workspace(&existing.id)
                .await
                .expect("existing versions")
                .is_empty()
        );
        assert_eq!(graph_file_count(&state, &existing.id).await, 0);
    }

    #[tokio::test]
    async fn create_workspace_commit_then_error_preserves_exact_initial_reference() {
        let (state, _dir) = test_state().await;
        let identity = state.store.reserve_workspace_identity();
        let mut candidate = VersionFileCandidate::from_graph(
            &identity.workspace_id,
            CandidateKind::Initial,
            &blank_graph(),
        )
        .expect("initial candidate");
        let graph_path = candidate
            .relative_path_text()
            .expect("initial candidate path")
            .to_owned();
        let graph_hash = candidate.graph_hash().to_owned();
        candidate
            .publish(&state.data_dir)
            .expect("publish candidate");
        let commit_identity = identity.clone();
        let store = state.store.clone();

        let error = commit_initial_candidate(&state, &mut candidate, async move {
            store
                .create_workspace_with_initial_version(CreateWorkspaceWithInitialVersion {
                    identity: commit_identity,
                    name: "Committed before synthetic error",
                    version_label: "Initial graph",
                    source: VersionSource::Manual,
                    graph_path: &graph_path,
                    graph_hash: &graph_hash,
                })
                .await
                .expect("real Store commit succeeds");
            Err(StoreError::StatementInvariant {
                operation: "synthetic_post_commit_outcome",
                expected_rows: 1,
                actual_rows: 0,
            })
        })
        .await
        .expect_err("synthetic outcome error remains visible");

        assert_eq!(error.status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        let workspaces = state.store.workspaces().await.expect("workspaces");
        assert_eq!(workspaces.len(), 1, "must not create a duplicate workspace");
        assert_eq!(workspaces[0].id, identity.workspace_id);
        assert_eq!(
            workspaces[0].cur_version_id.as_deref(),
            Some(identity.initial_version_id.as_str())
        );
        let version = state
            .store
            .version(&identity.initial_version_id)
            .await
            .expect("exact committed initial version");
        assert_eq!(version.graph_path, candidate.relative_path_text().unwrap());
        assert!(state.data_dir.join(&version.graph_path).exists());
        assert_eq!(graph_file_count(&state, &identity.workspace_id).await, 1);
    }

    #[tokio::test]
    async fn create_workspace_file_failure_has_no_database_side_effect() {
        let (mut state, dir) = test_state().await;
        let blocked_data_dir = dir.path().join("blocked-data-dir");
        tokio::fs::write(&blocked_data_dir, b"not a directory")
            .await
            .expect("write blocking file");
        state.data_dir = blocked_data_dir;
        let identity = state.store.reserve_workspace_identity();

        let error = initialize_workspace(&state, identity.clone(), "Rejected")
            .await
            .expect_err("candidate publication must fail");

        assert_eq!(error.status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(state.store.workspace(&identity.workspace_id).await.is_err());
        assert!(
            state
                .store
                .version(&identity.initial_version_id)
                .await
                .is_err()
        );
        assert!(
            state
                .store
                .workspaces()
                .await
                .expect("workspaces")
                .is_empty()
        );
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

    #[tokio::test]
    async fn set_workspace_provider_rejects_a_registered_but_unavailable_provider() {
        let (mut state, _dir) = test_state().await;
        state.provider_registry = ProviderRegistry::new(
            "mock",
            vec![
                RuntimeProvider::mock(),
                RuntimeProvider::unavailable("fal", "FAL_KEY is not configured"),
            ],
        );
        let workspace = create_workspace(
            State(state.clone()),
            Json(CreateWorkspaceRequest { name: None }),
        )
        .await
        .expect("create workspace")
        .0;

        let error = set_workspace_provider(
            Path(workspace.id.clone()),
            State(state.clone()),
            Json(SetWorkspaceProviderRequest {
                provider_id: "fal".to_owned(),
            }),
        )
        .await
        .expect_err("unavailable provider must not be persisted");

        assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
        assert_eq!(
            state
                .store
                .workspace(&workspace.id)
                .await
                .expect("workspace")
                .runtime_provider_id,
            None
        );
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

    async fn graph_file_count(state: &AppState, workspace_id: &str) -> usize {
        let directory = state
            .data_dir
            .join("workspaces")
            .join(workspace_id)
            .join("graphs");
        match tokio::fs::read_dir(directory).await {
            Ok(mut entries) => {
                let mut count = 0;
                while entries
                    .next_entry()
                    .await
                    .expect("read graph entry")
                    .is_some()
                {
                    count += 1;
                }
                count
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => panic!("read graph directory: {error}"),
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
