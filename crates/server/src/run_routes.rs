use axum::{
    Json,
    extract::{Path, State},
};
use serde::Serialize;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::workbench_payload::{
    OutputPayload, PendingConfirmationPayload, RunPayload, output_payload_from_artifact,
    run_payload_from_outcome,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunConfirmationResponse {
    run: RunPayload,
    outputs: Vec<OutputPayload>,
    pending_confirmation: Option<PendingConfirmationPayload>,
}

pub(crate) async fn confirm_run(
    Path((workspace_id, run_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<RunConfirmationResponse>, ApiError> {
    let run = state.store.run(&run_id).await.map_err(ApiError::store)?;
    if run.workspace_id != workspace_id {
        return Err(ApiError::not_found("run was not found in this workspace"));
    }

    let outcome = state
        .runner
        .confirm_run(&run_id)
        .await
        .map_err(ApiError::run)?;
    let costs = state
        .store
        .cost_ledger_for_run(&run_id)
        .await
        .map_err(ApiError::store)?;

    Ok(Json(RunConfirmationResponse {
        run: run_payload_from_outcome(&outcome, &costs),
        outputs: outcome
            .artifacts
            .iter()
            .map(output_payload_from_artifact)
            .collect(),
        pending_confirmation: None,
    }))
}

pub(crate) async fn hold_run(
    Path((workspace_id, run_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<RunConfirmationResponse>, ApiError> {
    let run = state.store.run(&run_id).await.map_err(ApiError::store)?;
    if run.workspace_id != workspace_id {
        return Err(ApiError::not_found("run was not found in this workspace"));
    }

    let outcome = state
        .runner
        .hold_run(&run_id)
        .await
        .map_err(ApiError::run)?;
    let costs = state
        .store
        .cost_ledger_for_run(&run_id)
        .await
        .map_err(ApiError::store)?;

    Ok(Json(RunConfirmationResponse {
        run: run_payload_from_outcome(&outcome, &costs),
        outputs: outcome
            .artifacts
            .iter()
            .map(output_payload_from_artifact)
            .collect(),
        pending_confirmation: None,
    }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::extract::{Path, State};
    use helixflow_agent::{
        AgentError, AgentSessionRequest, ValidatedAgentProposal, ValidatedAgentReply,
    };
    use helixflow_graph::WorkflowGraph;
    use helixflow_run::{AgentRunRequest, EventBus};
    use helixflow_store::{NewVersion, Store, VersionSource};

    use super::*;
    use crate::app_state::{AppState, WorkbenchAgent};

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

        assert_eq!(response.run.status, "succeeded");
        assert_eq!(response.pending_confirmation, None);
        let run = state.store.run(&pending.run.id).await.expect("run");
        assert_eq!(run.status, "succeeded");
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

    async fn state_with_workspace() -> (AppState, String, String, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store
            .create_workspace("Run route workspace")
            .await
            .expect("create workspace");
        let version = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Run graph",
                source: VersionSource::Manual,
                graph_path: "graphs/run.json",
                graph_hash: "sha256:run",
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
        (state, workspace.id, version.id, dir)
    }

    fn sample_graph() -> WorkflowGraph {
        WorkflowGraph {
            schema_version: 1,
            nodes: BTreeMap::new(),
            edges: Vec::new(),
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
