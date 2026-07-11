use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{HeaderValue, Response, header},
};
use helixflow_store::{ArtifactRecord, RunRecord};
use serde::Serialize;
use serde_json::Value;
use std::path::{Component, Path as FsPath, PathBuf};

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::workbench_payload::{
    OutputPreviewPayload, output_preview_from_artifact, safe_download_uri,
};
use crate::workspace_state::workspace_state_value;

pub(crate) async fn select_output(
    Path(output_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Value>, ApiError> {
    let artifact = artifact_by_id(&state, &output_id).await?;
    let selected = match latest_output_scope(&state, &artifact).await? {
        OutputSelectionScope::Run => state
            .store
            .select_run_artifact(&artifact.id)
            .await
            .map_err(ApiError::store)?,
        OutputSelectionScope::SweepGroup(group_id) => state
            .store
            .select_group_artifact(&artifact.id, &group_id)
            .await
            .map_err(ApiError::store)?,
    };

    Ok(Json(
        workspace_state_value(&state, &selected.workspace_id).await?,
    ))
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RejectOutputRequest {
    #[serde(default)]
    pub rerun: bool,
}

pub(crate) async fn accept_output(
    Path(output_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Value>, ApiError> {
    let artifact = artifact_by_id(&state, &output_id).await?;
    // Only outputs from the latest run are reviewable.
    latest_output_scope(&state, &artifact).await?;
    let updated = state
        .store
        .set_artifact_review_state(&artifact.id, &["pending"], "accepted")
        .await
        .map_err(ApiError::store)?
        .ok_or_else(|| ApiError::conflict("output is not pending review"))?;
    Ok(Json(
        workspace_state_value(&state, &updated.workspace_id).await?,
    ))
}

pub(crate) async fn reject_output(
    Path(output_id): Path<String>,
    State(state): State<AppState>,
    Json(request): Json<RejectOutputRequest>,
) -> Result<Json<Value>, ApiError> {
    let artifact = artifact_by_id(&state, &output_id).await?;
    latest_output_scope(&state, &artifact).await?;
    let allowed_states: &[&str] = if request.rerun {
        &["pending"]
    } else {
        &["pending", "rejected"]
    };
    let updated = state
        .store
        .set_artifact_review_state(&artifact.id, allowed_states, "rejected")
        .await
        .map_err(ApiError::store)?
        .ok_or_else(|| ApiError::conflict("output cannot be rejected"))?;
    if request.rerun {
        let run_id = updated
            .run_id
            .as_deref()
            .ok_or_else(|| ApiError::conflict("output is not attached to a run to rerun"))?;
        let retry = state
            .store
            .create_retry_run(run_id, true)
            .await
            .map_err(ApiError::store)?;
        // Reuse the self-repair cost gate: within budget it starts now,
        // otherwise it waits in `waiting_confirmation` for the user.
        state
            .runner
            .start_confirmed_run_within_budget(&retry.id)
            .await
            .map_err(ApiError::run)?;
    }
    Ok(Json(
        workspace_state_value(&state, &updated.workspace_id).await?,
    ))
}

pub(crate) async fn preview_output(
    Path(output_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<OutputPreviewPayload>, ApiError> {
    let artifact = artifact_by_id(&state, &output_id).await?;
    let title = artifact
        .node_id
        .clone()
        .unwrap_or_else(|| artifact.kind.clone());
    let preview = output_preview_from_artifact(&artifact, &title)
        .ok_or_else(|| ApiError::not_found("output has no safe preview"))?;
    Ok(Json(preview))
}

pub(crate) async fn download_output(
    Path(output_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<OutputDownloadPayload>, ApiError> {
    let artifact = artifact_by_id(&state, &output_id).await?;
    Ok(Json(OutputDownloadPayload {
        id: artifact.id.clone(),
        kind: artifact.kind.clone(),
        title: artifact
            .node_id
            .clone()
            .unwrap_or_else(|| artifact.kind.clone()),
        storage_uri: safe_download_uri(&artifact.id),
        mime: artifact.mime.clone(),
    }))
}

pub(crate) async fn artifact_content(
    Path(output_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Response<Body>, ApiError> {
    let artifact = artifact_by_id(&state, &output_id).await?;
    let path = artifact_content_path(&state.data_dir, &artifact.storage_uri)?;
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(ApiError::not_found("artifact content was not found"));
        }
        Err(err) => return Err(ApiError::io("read artifact content", err)),
    };
    let mut response = Response::new(Body::from(bytes));
    if let Some(mime) = artifact.mime.as_deref()
        && let Ok(value) = HeaderValue::from_str(mime)
    {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    Ok(response)
}

async fn artifact_by_id(state: &AppState, output_id: &str) -> Result<ArtifactRecord, ApiError> {
    match state.store.artifact(output_id).await {
        Ok(artifact) => Ok(artifact),
        Err(err) if err.is_not_found() => Err(ApiError::not_found("output was not found")),
        Err(err) => Err(ApiError::store(err)),
    }
}

fn artifact_content_path(data_dir: &FsPath, storage_uri: &str) -> Result<PathBuf, ApiError> {
    let relative = FsPath::new(storage_uri);
    let mut components = relative.components();
    match components.next() {
        Some(Component::Normal(prefix)) if prefix == "artifacts" => {}
        _ => return Err(ApiError::not_found("artifact content was not found")),
    }
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ApiError::bad_request("artifact storage path is invalid"));
    }
    Ok(data_dir.join(relative))
}

enum OutputSelectionScope {
    Run,
    SweepGroup(String),
}

async fn latest_output_scope(
    state: &AppState,
    artifact: &ArtifactRecord,
) -> Result<OutputSelectionScope, ApiError> {
    let latest_run = state
        .store
        .latest_workspace_run(&artifact.workspace_id)
        .await
        .map_err(ApiError::store)?
        .ok_or_else(|| ApiError::conflict("workspace has no latest run"))?;
    let Some(artifact_run_id) = artifact.run_id.as_deref() else {
        return Err(ApiError::conflict(
            "output is not attached to a workspace run",
        ));
    };

    if artifact_run_id == latest_run.id {
        return Ok(selection_scope_for_run(&latest_run));
    }

    if latest_run.trigger == "sweep"
        && let Some(group_id) = latest_run.group_id.as_deref()
    {
        let artifact_run = state
            .store
            .run(artifact_run_id)
            .await
            .map_err(ApiError::store)?;
        if artifact_run.workspace_id == artifact.workspace_id
            && artifact_run.group_id.as_deref() == Some(group_id)
        {
            return Ok(OutputSelectionScope::SweepGroup(group_id.to_owned()));
        }
    }

    Err(ApiError::conflict(
        "output is not attached to the latest workspace run",
    ))
}

fn selection_scope_for_run(run: &RunRecord) -> OutputSelectionScope {
    if run.trigger == "sweep"
        && let Some(group_id) = run.group_id.as_deref()
    {
        return OutputSelectionScope::SweepGroup(group_id.to_owned());
    }

    OutputSelectionScope::Run
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OutputDownloadPayload {
    id: String,
    kind: String,
    title: String,
    storage_uri: String,
    mime: Option<String>,
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
    use helixflow_graph::{GraphNode, WorkflowGraph};
    use helixflow_run::EventBus;
    use helixflow_store::{NewArtifact, NewRun, NewVersion, Store, VersionSource};
    use serde_json::json;

    use super::*;
    use crate::app_state::{AppState, WorkbenchAgent};

    #[tokio::test]
    async fn select_output_persists_single_selected_output_in_latest_run_state() {
        let (state, first_id, second_id, _dir) = state_with_two_latest_outputs().await;

        let body = select_output(Path(second_id.clone()), State(state.clone()))
            .await
            .expect("select output")
            .0;
        let outputs = body["outputs"].as_array().expect("outputs");

        assert_eq!(
            outputs
                .iter()
                .filter(|output| output["selected"] == true)
                .map(|output| output["id"].as_str().expect("id"))
                .collect::<Vec<_>>(),
            vec![second_id.as_str()]
        );
        assert!(outputs.iter().all(|output| {
            output["storageUri"]
                .as_str()
                .expect("storage uri")
                .starts_with("/api/artifacts/")
        }));
        assert_eq!(
            state
                .store
                .artifact(&first_id)
                .await
                .expect("first")
                .selected,
            false
        );
        assert_eq!(
            state
                .store
                .artifact(&second_id)
                .await
                .expect("second")
                .selected,
            true
        );
        let selected_output = outputs
            .iter()
            .find(|output| output["id"] == second_id)
            .expect("selected output");
        assert_eq!(selected_output["nodeId"], "video");
        assert!(
            outputs[1]["preview"]["content"]
                .as_str()
                .expect("preview")
                .starts_with("/api/artifacts/")
        );
        let state_json = serde_json::to_string(&body).expect("state json");
        assert!(!state_json.contains("/Users/"));
        assert!(!state_json.contains("raw provider text"));
    }

    #[tokio::test]
    async fn select_output_rejects_artifact_from_older_run() {
        let (state, old_artifact_id, _dir) = state_with_old_and_latest_outputs().await;

        let err = select_output(Path(old_artifact_id), State(state))
            .await
            .expect_err("old output should not be selectable");

        assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn select_output_allows_artifacts_from_latest_sweep_group() {
        let (state, first_id, second_id, _dir) = state_with_sweep_outputs().await;

        let body = select_output(Path(first_id.clone()), State(state.clone()))
            .await
            .expect("select sweep output")
            .0;
        let outputs = body["outputs"].as_array().expect("outputs");

        assert_eq!(outputs.len(), 2);
        assert_eq!(
            outputs
                .iter()
                .filter(|output| output["selected"] == true)
                .map(|output| output["id"].as_str().expect("id"))
                .collect::<Vec<_>>(),
            vec![first_id.as_str()]
        );
        assert_eq!(
            state
                .store
                .artifact(&second_id)
                .await
                .expect("second")
                .selected,
            false
        );
    }

    #[tokio::test]
    async fn preview_and_download_routes_hide_raw_storage_uri() {
        let (state, _first_id, second_id, _dir) = state_with_two_latest_outputs().await;

        let preview = preview_output(Path(second_id.clone()), State(state.clone()))
            .await
            .expect("preview output")
            .0;
        let download = download_output(Path(second_id.clone()), State(state))
            .await
            .expect("download output")
            .0;
        let download_json = serde_json::to_value(download).expect("download json");

        assert!(preview.content.contains("/api/artifacts/"));
        assert_eq!(
            download_json["storageUri"],
            format!("/api/artifacts/{second_id}/content")
        );
        assert!(!download_json.to_string().contains("/Users/"));
    }

    #[tokio::test]
    async fn accept_reject_transitions_review_state() {
        let (state, first_id, second_id, _dir) = state_with_two_latest_outputs().await;
        assert_eq!(
            state
                .store
                .artifact(&first_id)
                .await
                .expect("a")
                .review_state,
            "pending"
        );

        let _ = accept_output(Path(first_id.clone()), State(state.clone()))
            .await
            .expect("accept");
        assert_eq!(
            state
                .store
                .artifact(&first_id)
                .await
                .expect("a")
                .review_state,
            "accepted"
        );

        let _ = reject_output(
            Path(second_id.clone()),
            State(state.clone()),
            axum::Json(RejectOutputRequest::default()),
        )
        .await
        .expect("reject");
        assert_eq!(
            state
                .store
                .artifact(&second_id)
                .await
                .expect("b")
                .review_state,
            "rejected"
        );
    }

    #[tokio::test]
    async fn accept_then_reject_returns_conflict() {
        let (state, first_id, _second_id, _dir) = state_with_two_latest_outputs().await;
        let _ = accept_output(Path(first_id.clone()), State(state.clone()))
            .await
            .expect("accept");

        let err = reject_output(
            Path(first_id.clone()),
            State(state.clone()),
            axum::Json(RejectOutputRequest::default()),
        )
        .await
        .expect_err("accepted output is terminal");
        assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn review_stale_run_returns_conflict() {
        let (state, old_artifact_id, _dir) = state_with_old_and_latest_outputs().await;
        let err = accept_output(Path(old_artifact_id), State(state))
            .await
            .expect_err("stale output is not reviewable");
        assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn reject_with_rerun_derives_retry_run() {
        let (state, first_id, _second_id, _dir) = state_with_two_latest_outputs().await;
        let artifact = state.store.artifact(&first_id).await.expect("artifact");
        let parent_run_id = artifact.run_id.clone().expect("run id");

        let _ = reject_output(
            Path(first_id.clone()),
            State(state.clone()),
            axum::Json(RejectOutputRequest { rerun: true }),
        )
        .await
        .expect("reject with rerun");

        let latest = state
            .store
            .latest_workspace_run(&artifact.workspace_id)
            .await
            .expect("latest")
            .expect("run");
        assert_eq!(
            latest.parent_run_id.as_deref(),
            Some(parent_run_id.as_str())
        );
        assert_eq!(latest.attempt, 1);
        assert!(latest.force_rerun);
        assert!(latest.group_id.is_none());

        let err = reject_output(
            Path(first_id),
            State(state),
            axum::Json(RejectOutputRequest { rerun: true }),
        )
        .await
        .expect_err("repeated reject must not derive another run");
        assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
    }

    async fn state_with_two_latest_outputs() -> (AppState, String, String, tempfile::TempDir) {
        let (state, workspace_id, run_id, dir) = state_with_run().await;
        let first = create_artifact(
            &state.store,
            &workspace_id,
            &run_id,
            "image",
            "image",
            "workspace://outputs/run/image.png",
            true,
        )
        .await;
        let second = create_artifact(
            &state.store,
            &workspace_id,
            &run_id,
            "video",
            "video",
            "/Users/alice/raw-provider-output.mp4",
            false,
        )
        .await;
        (state, first.id, second.id, dir)
    }

    async fn state_with_old_and_latest_outputs() -> (AppState, String, tempfile::TempDir) {
        let (state, workspace_id, old_run_id, dir) = state_with_run().await;
        let old = create_artifact(
            &state.store,
            &workspace_id,
            &old_run_id,
            "old",
            "video",
            "workspace://outputs/old/video.mp4",
            true,
        )
        .await;
        state
            .store
            .create_run(NewRun {
                workspace_id: &workspace_id,
                version_id: &state
                    .store
                    .workspace(&workspace_id)
                    .await
                    .expect("workspace")
                    .cur_version_id
                    .expect("current version"),
                group_id: None,
                label: "Latest run",
                trigger: "manual",
                plan_json: None,
                estimate_json: None,
                status: "succeeded",
            })
            .await
            .expect("latest run");
        (state, old.id, dir)
    }

    async fn state_with_sweep_outputs() -> (AppState, String, String, tempfile::TempDir) {
        let (state, workspace_id, _manual_run_id, dir) = state_with_run().await;
        let version_id = state
            .store
            .workspace(&workspace_id)
            .await
            .expect("workspace")
            .cur_version_id
            .expect("current version");
        let first_run = state
            .store
            .create_run(NewRun {
                workspace_id: &workspace_id,
                version_id: &version_id,
                group_id: Some("sweep_route_test"),
                label: "Sweep one",
                trigger: "sweep",
                plan_json: None,
                estimate_json: None,
                status: "succeeded",
            })
            .await
            .expect("first sweep run");
        let second_run = state
            .store
            .create_run(NewRun {
                workspace_id: &workspace_id,
                version_id: &version_id,
                group_id: Some("sweep_route_test"),
                label: "Sweep two",
                trigger: "sweep",
                plan_json: None,
                estimate_json: None,
                status: "succeeded",
            })
            .await
            .expect("second sweep run");
        let first = create_artifact(
            &state.store,
            &workspace_id,
            &first_run.id,
            "first",
            "video",
            "workspace://outputs/sweep/first.mp4",
            false,
        )
        .await;
        let second = create_artifact(
            &state.store,
            &workspace_id,
            &second_run.id,
            "second",
            "video",
            "workspace://outputs/sweep/second.mp4",
            true,
        )
        .await;

        (state, first.id, second.id, dir)
    }

    async fn state_with_run() -> (AppState, String, String, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store
            .create_workspace("Artifact route workspace")
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
            serde_json::to_vec_pretty(&sample_graph()).expect("graph json"),
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
        let run = store
            .create_run(NewRun {
                workspace_id: &workspace.id,
                version_id: &version.id,
                group_id: None,
                label: "Latest run",
                trigger: "manual",
                plan_json: None,
                estimate_json: None,
                status: "succeeded",
            })
            .await
            .expect("create run");
        let state = AppState::with_store_agent(
            EventBus::new(16),
            store,
            data_dir.clone(),
            Arc::new(NoopWorkbenchAgent),
            data_dir.join("sessions"),
        );
        (state, workspace.id, run.id, dir)
    }

    async fn create_artifact(
        store: &Store,
        workspace_id: &str,
        run_id: &str,
        node_id: &str,
        kind: &str,
        storage_uri: &str,
        selected: bool,
    ) -> helixflow_store::ArtifactRecord {
        store
            .create_artifact(NewArtifact {
                workspace_id,
                run_id: Some(run_id),
                run_step_id: None,
                node_id: Some(node_id),
                kind,
                storage_uri,
                sha256: None,
                mime: Some(if kind == "video" {
                    "video/mp4"
                } else {
                    "image/png"
                }),
                width: Some(1080),
                height: Some(1920),
                duration_ms: Some(5000),
                selected,
                meta_json: Some(
                    r#"{"provider":"mock","capability":"text_to_video","raw_path":"/Users/alice/raw-provider-output.mp4","text":"raw provider text that must not enter workspace state"}"#,
                ),
            })
            .await
            .expect("create artifact")
    }

    fn sample_graph() -> WorkflowGraph {
        WorkflowGraph {
            schema_version: 1,
            nodes: BTreeMap::from([(
                "input".to_owned(),
                GraphNode {
                    node_type: "input.text".to_owned(),
                    title: "Input".to_owned(),
                    params: json!({ "text": "launch teaser" }),
                    pos: [0.0, 0.0],
                    size: None,
                },
            )]),
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
