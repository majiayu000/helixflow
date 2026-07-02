use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use helixflow_run::{ManualRunRequest, RunOutcome};
use helixflow_store::RunRecord;
use serde::Serialize;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::read_graph_file;
use crate::sweep_support::interrupt_target_run;
use crate::workbench_payload::{
    OutputPayload, PendingConfirmationPayload, RunPayload, output_payload_from_artifact,
    run_payload_from_outcome,
};
use tokio::sync::{Mutex, OwnedMutexGuard};

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
    let _queue_claim = claim_workspace_run_queue(&state, &workspace_id).await?;
    reject_active_workspace_run(&state, &workspace_id).await?;
    let workspace = state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    let provider = state.selected_provider_for_workspace(&workspace);
    if !state.provider_registry.provider_enabled(&provider) {
        return Err(ApiError::conflict(format!(
            "runtime provider `{provider}` is unavailable"
        )));
    }
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
        .start_manual_run(ManualRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Manual workbench run".to_owned(),
            provider,
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

    if is_sweep_run(&run) {
        return confirm_sweep_group(&state, &workspace_id, &run)
            .await
            .map(Json);
    }

    let outcome = state
        .runner
        .start_confirmed_run(&run_id)
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

    if is_sweep_run(&run) {
        return hold_sweep_group(&state, &workspace_id, &run)
            .await
            .map(Json);
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
    let target_run = interrupt_target_run(&state, &run).await?;
    if !is_interruptible_status(&target_run.status) {
        return Err(ApiError {
            status: StatusCode::CONFLICT,
            message: format!("run `{}` is not active", target_run.id),
        });
    }

    state
        .runner
        .interrupt_run(&target_run.id)
        .await
        .map_err(ApiError::run)?;
    response_from_run_id(&state, &target_run.id).await.map(Json)
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

async fn confirm_sweep_group(
    state: &AppState,
    workspace_id: &str,
    run: &RunRecord,
) -> Result<RunConfirmationResponse, ApiError> {
    let run_ids = sweep_group_run_ids(state, workspace_id, run).await?;
    let outcome = state
        .runner
        .start_confirmed_sweep(&run_ids, &run.id)
        .await
        .map_err(ApiError::run)?;
    let recommended = outcome
        .runs
        .iter()
        .find(|candidate| candidate.run.id == run.id)
        .or_else(|| outcome.runs.last())
        .ok_or_else(|| {
            ApiError::server_error(format!("sweep outcome omitted response run `{}`", run.id))
        })?;
    let costs = state
        .store
        .cost_ledger_for_run(&run.id)
        .await
        .map_err(ApiError::store)?;

    Ok(RunConfirmationResponse {
        run: run_payload_from_outcome(recommended, &costs),
        outputs: outcome
            .artifacts
            .iter()
            .map(output_payload_from_artifact)
            .collect(),
        pending_confirmation: None,
    })
}

async fn hold_sweep_group(
    state: &AppState,
    workspace_id: &str,
    run: &RunRecord,
) -> Result<RunConfirmationResponse, ApiError> {
    let run_ids = sweep_group_run_ids(state, workspace_id, run).await?;
    let mut target_outcome = None;
    for run_id in run_ids {
        let outcome = state
            .runner
            .hold_run(&run_id)
            .await
            .map_err(ApiError::run)?;
        if run_id == run.id {
            target_outcome = Some(outcome);
        }
    }
    let outcome = target_outcome.ok_or_else(|| {
        ApiError::server_error(format!("sweep hold omitted target run `{}`", run.id))
    })?;
    response_from_outcome(state, outcome).await
}

async fn sweep_group_run_ids(
    state: &AppState,
    workspace_id: &str,
    run: &RunRecord,
) -> Result<Vec<String>, ApiError> {
    let group_id = run
        .group_id
        .as_deref()
        .ok_or_else(|| ApiError::server_error("sweep run has no group id"))?;
    let runs = state
        .store
        .runs_for_group(group_id)
        .await
        .map_err(ApiError::store)?;
    if runs.is_empty() {
        return Err(ApiError::not_found("sweep group was not found"));
    }
    if runs
        .iter()
        .any(|candidate| candidate.workspace_id != workspace_id)
    {
        return Err(ApiError::not_found(
            "sweep group was not found in this workspace",
        ));
    }
    Ok(runs.into_iter().map(|candidate| candidate.id).collect())
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

async fn claim_workspace_run_queue(
    state: &AppState,
    workspace_id: &str,
) -> Result<OwnedMutexGuard<()>, ApiError> {
    let lock: Arc<Mutex<()>> = {
        let mut locks = state.run_queue_locks.lock().await;
        locks
            .entry(workspace_id.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    lock.try_lock_owned().map_err(|_| ApiError {
        status: StatusCode::CONFLICT,
        message: format!("workspace already has a queue request in flight: {workspace_id}"),
    })
}

fn is_active_status(status: &str) -> bool {
    matches!(
        status,
        "queued" | "estimating" | "waiting_confirmation" | "running"
    )
}

fn is_sweep_run(run: &RunRecord) -> bool {
    run.trigger == "sweep" && run.group_id.is_some()
}

fn is_interruptible_status(status: &str) -> bool {
    matches!(status, "queued" | "estimating" | "running")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use axum::extract::{Path, State};
    use helixflow_graph::{GraphEdge, GraphNode, WorkflowGraph};
    use helixflow_run::{AgentRunRequest, EventBus, SweepPlan, SweepVariant};
    use helixflow_store::{NewVersion, Store, VersionSource};
    use serde_json::json;

    use super::*;
    use crate::app_state::AppState;
    use crate::test_support::FailingWorkbenchAgent;
    use crate::test_wait::{wait_for_actual_cost, wait_for_run_status};

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
                provider: "mock".to_owned(),
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

        assert_eq!(response.run.status, "running");
        assert_eq!(response.pending_confirmation, None);
        wait_for_run_status(&state.store, &pending.run.id, "succeeded").await;
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
                provider: "mock".to_owned(),
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
                provider: "mock".to_owned(),
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
    async fn confirm_route_executes_sweep_group_and_selects_recommendation() {
        let (state, workspace_id, version_id, _dir) = state_with_workspace().await;
        let pending = state
            .runner
            .request_sweep_plan(SweepPlan {
                workspace_id: workspace_id.clone(),
                version_id,
                label: "Seed sweep".to_owned(),
                provider: "mock".to_owned(),
                variants: vec![
                    SweepVariant {
                        label: "seed 101".to_owned(),
                        graph: executable_graph(),
                    },
                    SweepVariant {
                        label: "seed 202".to_owned(),
                        graph: executable_graph(),
                    },
                ],
            })
            .await
            .expect("request sweep");
        let recommended_run_id = pending.runs[1].run.id.clone();

        let response = confirm_run(
            Path((workspace_id.clone(), recommended_run_id.clone())),
            State(state.clone()),
        )
        .await
        .expect("confirm sweep response")
        .0;

        assert_eq!(response.run.id, recommended_run_id);
        assert_eq!(response.run.status, "queued");
        assert!(response.outputs.is_empty());
        for pending_run in pending.runs {
            let run = wait_for_run_status(&state.store, &pending_run.run.id, "succeeded").await;
            wait_for_actual_cost(&state.store, &run.id).await;
            let ledger = state
                .store
                .cost_ledger_for_run(&run.id)
                .await
                .expect("cost ledger");
            assert!(ledger.iter().any(|entry| entry.estimated));
            assert!(ledger.iter().any(|entry| !entry.estimated));
        }
    }

    #[tokio::test]
    async fn hold_route_interrupts_all_waiting_sweep_runs() {
        let (state, workspace_id, version_id, _dir) = state_with_workspace().await;
        let pending = state
            .runner
            .request_sweep_plan(SweepPlan {
                workspace_id: workspace_id.clone(),
                version_id,
                label: "Seed sweep".to_owned(),
                provider: "mock".to_owned(),
                variants: vec![
                    SweepVariant {
                        label: "seed 101".to_owned(),
                        graph: executable_graph(),
                    },
                    SweepVariant {
                        label: "seed 202".to_owned(),
                        graph: executable_graph(),
                    },
                ],
            })
            .await
            .expect("request sweep");
        let target_run_id = pending.runs[1].run.id.clone();

        let response = hold_run(
            Path((workspace_id.clone(), target_run_id.clone())),
            State(state.clone()),
        )
        .await
        .expect("hold sweep response")
        .0;

        assert_eq!(response.run.id, target_run_id);
        assert_eq!(response.run.status, "interrupted");
        assert_eq!(response.pending_confirmation, None);
        for pending_run in pending.runs {
            let run = state
                .store
                .run(&pending_run.run.id)
                .await
                .expect("sweep run");
            assert_eq!(run.status, "interrupted");
        }
    }

    #[tokio::test]
    async fn queue_route_executes_current_workspace_graph() {
        let (state, workspace_id, _version_id, _dir) = state_with_workspace().await;
        write_current_graph(&state).await;

        let response = queue_workspace_run(Path(workspace_id.clone()), State(state.clone()))
            .await
            .expect("queue response")
            .0;

        assert_eq!(response.run.status, "queued");
        assert_eq!(response.pending_confirmation, None);
        assert_eq!(response.run.steps.len(), 1);
        let run = wait_for_run_status(&state.store, &response.run.id, "succeeded").await;
        assert_eq!(run.trigger, "manual");
    }

    #[tokio::test]
    async fn queue_route_rejects_empty_current_graph() {
        let (state, workspace_id, _version_id, _dir) = state_with_workspace().await;
        write_graph(&state, &empty_graph()).await;

        let err = queue_workspace_run(Path(workspace_id), State(state))
            .await
            .expect_err("empty graph should not be queued");

        assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(err.message.contains("no executable steps"));
    }

    #[tokio::test]
    async fn queue_route_rejects_concurrent_in_flight_queue_request() {
        let (state, workspace_id, _version_id, _dir) = state_with_workspace().await;
        write_current_graph(&state).await;
        let _claim = claim_workspace_run_queue(&state, &workspace_id)
            .await
            .expect("claim queue");

        let err = queue_workspace_run(Path(workspace_id), State(state))
            .await
            .expect_err("second queue should fail while first is in flight");

        assert_eq!(err.status, StatusCode::CONFLICT);
        assert!(err.message.contains("queue request in flight"));
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
                provider: "mock".to_owned(),
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
                provider: "mock".to_owned(),
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
            Arc::new(FailingWorkbenchAgent),
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

    fn empty_graph() -> WorkflowGraph {
        WorkflowGraph {
            schema_version: 1,
            nodes: BTreeMap::new(),
            edges: Vec::new(),
        }
    }

    fn executable_graph() -> WorkflowGraph {
        WorkflowGraph {
            schema_version: 1,
            nodes: BTreeMap::from([
                (
                    "video".to_owned(),
                    GraphNode {
                        node_type: "video.text_to_video".to_owned(),
                        title: "Video render".to_owned(),
                        params: json!({
                            "prompt": "clean product shot",
                            "duration_sec": 4,
                            "aspect_ratio": "9:16"
                        }),
                        pos: [0.0, 0.0],
                    },
                ),
                (
                    "save".to_owned(),
                    GraphNode {
                        node_type: "output.save".to_owned(),
                        title: "Save".to_owned(),
                        params: json!({}),
                        pos: [240.0, 0.0],
                    },
                ),
            ]),
            edges: vec![GraphEdge {
                from: ["video".to_owned(), "video".to_owned()],
                to: ["save".to_owned(), "artifact".to_owned()],
                edge_type: "artifact".to_owned(),
            }],
        }
    }

    async fn write_current_graph(state: &AppState) {
        write_graph(state, &sample_graph()).await;
    }

    async fn write_graph(state: &AppState, graph: &WorkflowGraph) {
        let graph_path = state.data_dir.join("graphs/run.json");
        tokio::fs::create_dir_all(graph_path.parent().expect("graph parent"))
            .await
            .expect("create graph dir");
        tokio::fs::write(
            graph_path,
            serde_json::to_vec_pretty(graph).expect("encode graph"),
        )
        .await
        .expect("write graph");
    }
}
