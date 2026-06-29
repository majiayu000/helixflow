use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use helixflow_run::{ManualRunRequest, RunOutcome};
use serde::Serialize;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::read_graph_file;
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

pub(crate) async fn queue_workspace_run(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<RunConfirmationResponse>, ApiError> {
    reject_active_workspace_run(&state, &workspace_id).await?;
    let workspace = state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    let version_id = workspace.cur_version_id.ok_or_else(|| ApiError {
        status: StatusCode::CONFLICT,
        message: "workspace has no current version to run".to_owned(),
    })?;
    let version = state
        .store
        .version(&version_id)
        .await
        .map_err(ApiError::store)?;
    let graph = read_graph_file(&state.data_dir, &version.graph_path).await?;

    let outcome = state
        .runner
        .execute_manual_run(ManualRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Manual workbench run".to_owned(),
            graph,
        })
        .await
        .map_err(ApiError::run)?;

    Ok(Json(response_from_outcome(&state, outcome).await?))
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

pub(crate) async fn interrupt_active_run(
    Path(run_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<RunConfirmationResponse>, ApiError> {
    let run = state.store.run(&run_id).await.map_err(ApiError::store)?;
    if !is_interruptible_status(&run.status) {
        return Err(ApiError {
            status: StatusCode::CONFLICT,
            message: format!("run `{run_id}` is not active"),
        });
    }

    state
        .runner
        .interrupt_run(&run_id)
        .await
        .map_err(ApiError::run)?;
    response_from_run_id(&state, &run_id).await.map(Json)
}

async fn response_from_outcome(
    state: &AppState,
    outcome: RunOutcome,
) -> Result<RunConfirmationResponse, ApiError> {
    let costs = state
        .store
        .cost_ledger_for_run(&outcome.run.id)
        .await
        .map_err(ApiError::store)?;

    Ok(RunConfirmationResponse {
        run: run_payload_from_outcome(&outcome, &costs),
        outputs: outcome
            .artifacts
            .iter()
            .map(output_payload_from_artifact)
            .collect(),
        pending_confirmation: None,
    })
}

async fn response_from_run_id(
    state: &AppState,
    run_id: &str,
) -> Result<RunConfirmationResponse, ApiError> {
    let outcome = RunOutcome {
        run: state.store.run(run_id).await.map_err(ApiError::store)?,
        steps: state
            .store
            .run_steps(run_id)
            .await
            .map_err(ApiError::store)?,
        artifacts: state
            .store
            .run_artifacts(run_id)
            .await
            .map_err(ApiError::store)?,
    };
    response_from_outcome(state, outcome).await
}

async fn reject_active_workspace_run(state: &AppState, workspace_id: &str) -> Result<(), ApiError> {
    let latest = state
        .store
        .latest_workspace_run(workspace_id)
        .await
        .map_err(ApiError::store)?;
    if let Some(run) = latest
        && is_active_status(&run.status)
    {
        return Err(ApiError {
            status: StatusCode::CONFLICT,
            message: format!("workspace already has active run `{}`", run.id),
        });
    }
    Ok(())
}

fn is_active_status(status: &str) -> bool {
    matches!(
        status,
        "queued" | "estimating" | "waiting_confirmation" | "running"
    )
}

fn is_interruptible_status(status: &str) -> bool {
    matches!(status, "queued" | "estimating" | "running")
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
    use helixflow_graph::{GraphNode, WorkflowGraph};
    use helixflow_run::{AgentRunRequest, EventBus};
    use helixflow_store::{NewVersion, Store, VersionSource};
    use serde_json::json;

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

    #[tokio::test]
    async fn queue_route_executes_current_workspace_graph() {
        let (state, workspace_id, _version_id, _dir) = state_with_workspace().await;
        write_current_graph(&state).await;

        let response = queue_workspace_run(Path(workspace_id.clone()), State(state.clone()))
            .await
            .expect("queue response")
            .0;

        assert_eq!(response.run.status, "succeeded");
        assert_eq!(response.pending_confirmation, None);
        assert_eq!(response.run.steps.len(), 1);
        let run = state
            .store
            .latest_workspace_run(&workspace_id)
            .await
            .expect("latest run")
            .expect("run");
        assert_eq!(run.trigger, "manual");
        assert_eq!(run.status, "succeeded");
    }

    #[tokio::test]
    async fn queue_route_rejects_waiting_confirmation_double_submit() {
        let (state, workspace_id, version_id, _dir) = state_with_workspace().await;
        write_current_graph(&state).await;
        let pending = state
            .runner
            .request_agent_run(AgentRunRequest {
                workspace_id: workspace_id.clone(),
                version_id,
                group_id: None,
                label: "Pending run".to_owned(),
                graph: sample_graph(),
            })
            .await
            .expect("request run");

        let err = queue_workspace_run(Path(workspace_id), State(state))
            .await
            .expect_err("active run should block queue");

        assert_eq!(err.status, StatusCode::CONFLICT);
        assert!(err.message.contains(&pending.run.id));
    }

    #[tokio::test]
    async fn interrupt_route_rejects_waiting_confirmation_run() {
        let (state, workspace_id, version_id, _dir) = state_with_workspace().await;
        let pending = state
            .runner
            .request_agent_run(AgentRunRequest {
                workspace_id,
                version_id,
                group_id: None,
                label: "Pending run".to_owned(),
                graph: sample_graph(),
            })
            .await
            .expect("request run");

        let err = interrupt_active_run(Path(pending.run.id), State(state))
            .await
            .expect_err("waiting confirmation is not active interrupt");

        assert_eq!(err.status, StatusCode::CONFLICT);
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
            nodes: BTreeMap::from([(
                "text".to_owned(),
                GraphNode {
                    node_type: "input.text".to_owned(),
                    title: "Text".to_owned(),
                    params: json!({ "text": "launch teaser" }),
                    pos: [0.0, 0.0],
                },
            )]),
            edges: Vec::new(),
        }
    }

    async fn write_current_graph(state: &AppState) {
        let graph_path = state.data_dir.join("graphs/run.json");
        tokio::fs::create_dir_all(graph_path.parent().expect("graph parent"))
            .await
            .expect("create graph dir");
        tokio::fs::write(
            graph_path,
            serde_json::to_vec_pretty(&sample_graph()).expect("encode graph"),
        )
        .await
        .expect("write graph");
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
