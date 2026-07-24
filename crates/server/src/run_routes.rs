use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_run::{ManualRunRequest, RunOutcome};
use helixflow_store::RunRecord;
use serde::{Deserialize, Serialize};

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::sweep_support::{
    interrupt_target_run, parse_run_confirmation_threshold_usd, requires_run_confirmation,
    run_confirmation_threshold_config,
};
use crate::version_file_consistency::read_version_graph;
use crate::workbench_payload::{
    OutputPayload, PendingConfirmationPayload, RunPayload, output_payload_from_artifact,
    pending_confirmation_from_pending, run_payload_from_outcome, run_payload_from_pending,
};
use tokio::sync::{Mutex, OwnedMutexGuard};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunConfirmationResponse {
    pub(crate) run: RunPayload,
    pub(crate) outputs: Vec<OutputPayload>,
    pub(crate) pending_confirmation: Option<PendingConfirmationPayload>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueueRunRequest {
    force_rerun: bool,
}

pub(crate) async fn queue_workspace_run(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
    body: Option<Json<QueueRunRequest>>,
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
    let version_id = workspace
        .cur_version_id
        .ok_or_else(|| ApiError::conflict("workspace has no current version to run"))?;
    let version = state
        .store
        .version(&version_id)
        .await
        .map_err(ApiError::store)?;
    let graph = read_version_graph(&state.data_dir, &version)
        .await
        .map_err(|error| ApiError::server_error(error.to_string()))?;
    crate::capability_preflight::preflight_provider_capabilities(&state, &provider, &graph)?;
    let force_rerun = body.map(|Json(body)| body.force_rerun).unwrap_or(false);

    let threshold =
        parse_run_confirmation_threshold_usd(run_confirmation_threshold_config()?.as_deref())
            .map_err(ApiError::server_error)?;
    let pending = state
        .runner
        .prepare_manual_run(ManualRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Manual workbench run".to_owned(),
            provider,
            graph,
            force_rerun,
        })
        .await
        .map_err(ApiError::run)?;
    let requires_confirmation = requires_run_confirmation(&pending.estimate, threshold);
    state
        .runner
        .announce_run_requested(&pending, requires_confirmation)
        .await
        .map_err(ApiError::run)?;

    if requires_confirmation {
        return Ok(Json(RunConfirmationResponse {
            run: run_payload_from_pending(&pending),
            outputs: Vec::new(),
            pending_confirmation: Some(pending_confirmation_from_pending(&pending)),
        }));
    }

    let outcome = state
        .runner
        .start_confirmed_run(&pending.run.id)
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
        return Err(ApiError::conflict(format!(
            "run `{}` is not active",
            target_run.id
        )));
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
        return Err(ApiError::conflict(format!(
            "workspace already has active run `{}`",
            run.id
        )));
    }
    Ok(())
}

pub(crate) async fn claim_workspace_run_queue(
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
    lock.try_lock_owned().map_err(|_| {
        ApiError::conflict(format!(
            "workspace already has a queue request in flight: {workspace_id}"
        ))
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
