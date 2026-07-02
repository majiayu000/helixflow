use helixflow_store::RunRecord;

use crate::api_error::ApiError;
use crate::app_state::AppState;

pub(crate) async fn visible_workspace_run(
    state: &AppState,
    latest_run: Option<RunRecord>,
) -> Result<Option<RunRecord>, ApiError> {
    let Some(run) = latest_run else {
        return Ok(None);
    };
    if run.trigger != "sweep" {
        return Ok(Some(run));
    }
    let Some(group_id) = run.group_id.as_deref() else {
        return Ok(Some(run));
    };
    if !matches!(
        run.status.as_str(),
        "queued" | "estimating" | "waiting_confirmation" | "running"
    ) {
        return Ok(Some(run));
    }

    let runs = state
        .store
        .runs_for_group(group_id)
        .await
        .map_err(ApiError::store)?;
    if let Some(running) = runs
        .iter()
        .find(|candidate| candidate.status == "running")
        .cloned()
    {
        return Ok(Some(running));
    }
    for candidate in runs.iter().filter(|candidate| candidate.status == "queued") {
        if state.runner.has_interrupt(&candidate.id).await {
            return Ok(Some(candidate.clone()));
        }
    }
    Ok(Some(run))
}
