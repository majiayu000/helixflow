use axum::{
    Json,
    extract::{Path as AxumPath, Query, State},
};
use helixflow_run::RunEventEnvelope;
use serde::{Deserialize, Serialize};

use crate::api_error::ApiError;
use crate::app_state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceEventsQuery {
    after_seq: Option<i64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct WorkspaceEventsPayload {
    events: Vec<RunEventEnvelope>,
}

pub(crate) async fn workspace_events(
    AxumPath(workspace_id): AxumPath<String>,
    Query(query): Query<WorkspaceEventsQuery>,
    State(state): State<AppState>,
) -> Result<Json<WorkspaceEventsPayload>, ApiError> {
    state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    let after_seq = query.after_seq.unwrap_or(0);
    if after_seq < 0 {
        return Err(ApiError::bad_request("afterSeq must be non-negative"));
    }

    let Some(run) = state
        .store
        .latest_workspace_run(&workspace_id)
        .await
        .map_err(ApiError::store)?
    else {
        return Ok(Json(WorkspaceEventsPayload { events: Vec::new() }));
    };
    let records = state
        .store
        .run_events(&run.id)
        .await
        .map_err(ApiError::store)?;
    let mut events = Vec::new();
    for record in records.into_iter().filter(|record| record.seq > after_seq) {
        let data = serde_json::from_str(&record.data_json).map_err(|err| {
            ApiError::server_error(format!(
                "run event `{}` contains invalid JSON: {err}",
                record.id
            ))
        })?;
        events.push(RunEventEnvelope {
            workspace_id: workspace_id.clone(),
            run_id: run.id.clone(),
            seq: record.seq,
            server_time: record.created_at,
            ev: record.ev,
            data,
        });
    }
    Ok(Json(WorkspaceEventsPayload { events }))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::extract::{Path as AxumPath, Query, State};
    use helixflow_run::EventBus;
    use helixflow_store::{NewRun, NewVersion, Store, VersionSource};

    use super::*;
    use crate::app_state::AppState;
    use crate::graph_files::write_json_file;
    use crate::test_support::FailingWorkbenchAgent;

    #[tokio::test]
    async fn workspace_events_returns_latest_run_events_after_seq() {
        let (state, workspace_id, _dir) = state_with_run_events().await;

        let body = workspace_events(
            AxumPath(workspace_id),
            Query(WorkspaceEventsQuery { after_seq: Some(1) }),
            State(state),
        )
        .await
        .expect("events")
        .0;

        assert_eq!(body.events.len(), 1);
        assert_eq!(body.events[0].seq, 2);
        assert_eq!(body.events[0].ev, "run.succeeded");
    }

    async fn state_with_run_events() -> (AppState, String, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store
            .create_workspace("Events workspace")
            .await
            .expect("workspace");
        let graph_path = std::path::PathBuf::from("graphs/events.json");
        let graph_hash = write_json_file(
            &data_dir,
            &graph_path,
            &serde_json::json!({"schema_version":1,"nodes":{},"edges":[]}),
            "write graph",
        )
        .await
        .expect("write graph");
        let version = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Events graph",
                source: VersionSource::Manual,
                graph_path: graph_path.to_string_lossy().as_ref(),
                graph_hash: &graph_hash,
                parent_id: None,
                semantics_json: None,
            })
            .await
            .expect("version");
        let run = store
            .create_run(NewRun {
                workspace_id: &workspace.id,
                version_id: &version.id,
                group_id: None,
                label: "Run",
                trigger: "manual",
                plan_json: None,
                estimate_json: None,
                status: "running",
            })
            .await
            .expect("run");
        store
            .append_run_event(
                &run.id,
                "node.state",
                r#"{"node_id":"video","state":"running"}"#,
            )
            .await
            .expect("event 1");
        store
            .append_run_event(&run.id, "run.succeeded", r#"{}"#)
            .await
            .expect("event 2");
        let state = AppState::with_store_agent(
            EventBus::new(16),
            store,
            data_dir.clone(),
            Arc::new(FailingWorkbenchAgent),
            data_dir.join("sessions"),
        );
        (state, workspace.id, dir)
    }
}
