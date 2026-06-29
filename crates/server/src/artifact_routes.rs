use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_store::ArtifactRecord;
use serde::Serialize;
use serde_json::Value;

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
    ensure_latest_run_artifact(&state, &artifact).await?;
    let selected = state
        .store
        .select_run_artifact(&artifact.id)
        .await
        .map_err(ApiError::store)?;

    Ok(Json(
        workspace_state_value(&state, &selected.workspace_id).await?,
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

async fn artifact_by_id(state: &AppState, output_id: &str) -> Result<ArtifactRecord, ApiError> {
    match state.store.artifact(output_id).await {
        Ok(artifact) => Ok(artifact),
        Err(err) if err.is_not_found() => Err(ApiError::not_found("output was not found")),
        Err(err) => Err(ApiError::store(err)),
    }
}

async fn ensure_latest_run_artifact(
    state: &AppState,
    artifact: &ArtifactRecord,
) -> Result<(), ApiError> {
    let latest_run = state
        .store
        .latest_workspace_run(&artifact.workspace_id)
        .await
        .map_err(ApiError::store)?
        .ok_or_else(|| ApiError::conflict("workspace has no latest run"))?;
    if artifact.run_id.as_deref() != Some(latest_run.id.as_str()) {
        return Err(ApiError::conflict(
            "output is not attached to the latest workspace run",
        ));
    }
    Ok(())
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
                .starts_with("/api/outputs/")
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
        assert!(
            outputs[1]["preview"]["content"]
                .as_str()
                .expect("preview")
                .contains("Artifact")
        );
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

        assert!(preview.content.contains("/api/outputs/"));
        assert_eq!(
            download_json["storageUri"],
            format!("/api/outputs/{second_id}/download")
        );
        assert!(!download_json.to_string().contains("/Users/"));
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
                meta_json: Some(r#"{"provider":"mock","capability":"text_to_video"}"#),
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
