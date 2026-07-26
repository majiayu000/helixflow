use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_graph::WorkflowGraph;
use helixflow_store::{NewVersion, VersionRecord, VersionSource, WorkspaceRecord};
use serde_json::Value;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::version_file_consistency::read_version_graph;
use crate::version_semantics::derive_semantics_json;
use crate::workspace_state::workspace_state_value;

pub(crate) async fn export_workflow_version(
    Path(version_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<WorkflowGraph>, ApiError> {
    let version = match state.store.version(&version_id).await {
        Ok(version) => version,
        Err(err) if err.is_not_found() => return Err(ApiError::not_found("version was not found")),
        Err(err) => return Err(ApiError::store(err)),
    };
    let graph = read_version_graph(&state.data_dir, &version)
        .await
        .map_err(|error| ApiError::server_error(error.to_string()))?;
    ensure_exportable_graph(&graph)?;
    Ok(Json(graph))
}

pub(crate) async fn undo_workspace_version(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Value>, ApiError> {
    let (workspace, versions, current) = workspace_version_context(&state, &workspace_id).await?;
    let target = undo_target(&versions, &current)?;
    create_restore_version(&state, &workspace, &current, &target, RestoreAction::Undo).await?;

    Ok(Json(workspace_state_value(&state, &workspace_id).await?))
}

pub(crate) async fn restore_workspace_version(
    Path((workspace_id, version_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<Value>, ApiError> {
    let (workspace, versions, current) = workspace_version_context(&state, &workspace_id).await?;
    if version_id == current.id {
        return Err(ApiError::conflict("version is already current"));
    }
    let target = versions
        .iter()
        .find(|version| version.id == version_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found("version was not found in this workspace"))?;
    create_restore_version(
        &state,
        &workspace,
        &current,
        &target,
        RestoreAction::Restore,
    )
    .await?;

    Ok(Json(workspace_state_value(&state, &workspace_id).await?))
}

async fn workspace_version_context(
    state: &AppState,
    workspace_id: &str,
) -> Result<(WorkspaceRecord, Vec<VersionRecord>, VersionRecord), ApiError> {
    let workspace = state
        .store
        .workspace(workspace_id)
        .await
        .map_err(ApiError::store)?;
    let versions = state
        .store
        .versions_for_workspace(workspace_id)
        .await
        .map_err(ApiError::store)?;
    let current_id = workspace
        .cur_version_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("workspace has no current version"))?;
    let current = versions
        .iter()
        .find(|version| version.id == current_id)
        .cloned()
        .ok_or_else(|| ApiError::conflict("workspace current version is missing from history"))?;

    Ok((workspace, versions, current))
}

fn undo_target(
    versions: &[VersionRecord],
    current: &VersionRecord,
) -> Result<VersionRecord, ApiError> {
    if let Some(parent_id) = current.parent_id.as_deref()
        && let Some(parent) = versions.iter().find(|version| version.id == parent_id)
    {
        return Ok(parent.clone());
    }

    let current_index = versions
        .iter()
        .position(|version| version.id == current.id)
        .ok_or_else(|| ApiError::conflict("workspace current version is missing from history"))?;
    if current_index == 0 {
        return Err(ApiError::conflict("workspace has no undo target"));
    }
    Ok(versions[current_index - 1].clone())
}

async fn create_restore_version(
    state: &AppState,
    workspace: &WorkspaceRecord,
    current: &VersionRecord,
    target: &VersionRecord,
    action: RestoreAction,
) -> Result<VersionRecord, ApiError> {
    let target_graph = read_version_graph(&state.data_dir, target)
        .await
        .map_err(|error| ApiError::server_error(error.to_string()))?;
    let semantics_json = derive_semantics_json(target, &target_graph, &target_graph)?;
    let label = match action {
        RestoreAction::Undo => format!("Undo to {}", target.label),
        RestoreAction::Restore => format!("Restore {}", target.label),
    };
    state
        .store
        .create_version_after(
            NewVersion {
                workspace_id: &workspace.id,
                label: &label,
                source: VersionSource::Restore,
                graph_path: &target.graph_path,
                graph_hash: &target.graph_hash,
                parent_id: Some(&current.id),
                semantics_json: semantics_json.as_deref(),
            },
            &current.id,
        )
        .await
        .map_err(ApiError::store)
}

#[derive(Debug, Clone, Copy)]
enum RestoreAction {
    Undo,
    Restore,
}

fn ensure_exportable_graph(graph: &WorkflowGraph) -> Result<(), ApiError> {
    let value = serde_json::to_value(graph).map_err(|err| {
        ApiError::server_error(format!("encode workflow graph for export: {err}"))
    })?;
    ensure_exportable_json(&value, "$")
}

fn ensure_exportable_json(value: &Value, path: &str) -> Result<(), ApiError> {
    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                let nested_path = format!("{path}.{key}");
                if is_non_exportable_key(key) {
                    return Err(ApiError::bad_request(format!(
                        "workflow graph contains non-exportable field `{nested_path}`"
                    )));
                }
                ensure_exportable_json(nested, &nested_path)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for (index, nested) in items.iter().enumerate() {
                ensure_exportable_json(nested, &format!("{path}[{index}]"))?;
            }
            Ok(())
        }
        Value::String(text) if looks_like_local_absolute_path(text) => Err(ApiError::bad_request(
            format!("workflow graph contains non-exportable local path at `{path}`"),
        )),
        _ => Ok(()),
    }
}

fn is_non_exportable_key(key: &str) -> bool {
    let compact = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect::<String>();

    compact.contains("secret")
        || compact.contains("password")
        || compact.contains("credential")
        || compact.contains("privatekey")
        || compact.contains("apikey")
        || compact == "token"
        || compact.ends_with("token")
        || compact.contains("authorization")
        || compact.contains("authheader")
        || compact.contains("requestheader")
        || compact == "header"
        || compact.ends_with("header")
        || compact == "headers"
        || compact.ends_with("headers")
        || compact.contains("cookie")
        || compact == "runid"
        || compact.ends_with("runid")
        || compact == "sessionid"
        || compact.ends_with("sessionid")
        || compact == "traceid"
        || compact.ends_with("traceid")
        || compact == "providerrequestid"
        || compact.ends_with("providerrequestid")
        || compact.contains("runtimemetadata")
        || compact.contains("runtimeonly")
}

fn looks_like_local_absolute_path(value: &str) -> bool {
    let trimmed = value.trim();
    (trimmed.starts_with('/') && !trimmed.starts_with("//"))
        || trimmed.starts_with("~/")
        || trimmed.starts_with('\\')
        || trimmed.as_bytes().get(1) == Some(&b':')
            && matches!(trimmed.as_bytes().get(2), Some(b'\\' | b'/'))
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
    use helixflow_graph::{GraphEdge, GraphNode, WorkflowGraph};
    use helixflow_run::EventBus;
    use helixflow_store::{NewProposal, NewVersion, Store, VersionSource};
    use serde_json::json;

    use super::*;
    use crate::app_state::{AppState, WorkbenchAgent};
    use crate::graph_files::graph_hash;

    #[tokio::test]
    async fn export_workflow_version_returns_current_graph_when_proposal_is_pending() {
        let (state, version_id, _dir) = state_with_current_graph_and_pending_proposal().await;

        let body = export_workflow_version(Path(version_id), State(state))
            .await
            .expect("export version")
            .0;

        assert_eq!(
            body.nodes["video"].params["duration_sec"],
            json!(5),
            "export must return current version graph, not pending proposal preview"
        );
        let encoded = serde_json::to_string(&body).expect("export json");
        assert!(!encoded.contains("api_key"));
        assert!(!encoded.contains("/Users/"));
    }

    #[tokio::test]
    async fn export_workflow_version_rejects_unsafe_graph_params() {
        let (state, version_id, _dir) = state_with_graph(unsafe_graph()).await;

        let err = export_workflow_version(Path(version_id), State(state))
            .await
            .expect_err("unsafe export should fail");

        assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
        assert!(err.message.contains("non-exportable field"));
        assert!(!err.message.contains("sk_live"));
    }

    #[tokio::test]
    async fn undo_workspace_version_creates_restore_version_from_parent_graph() {
        let (state, workspace_id, base_version_id, current_version_id, _dir) =
            state_with_two_versions().await;

        let body = undo_workspace_version(Path(workspace_id.clone()), State(state.clone()))
            .await
            .expect("undo version")
            .0;
        let restored_version_id = body["workspace"]["versionId"].as_str().expect("version id");
        let restored_version = state
            .store
            .version(restored_version_id)
            .await
            .expect("restored version");
        let base_version = state
            .store
            .version(&base_version_id)
            .await
            .expect("base version");

        assert_ne!(restored_version_id, base_version_id);
        assert_ne!(restored_version_id, current_version_id);
        assert_eq!(restored_version.source, "restore");
        assert_eq!(
            restored_version.parent_id.as_deref(),
            Some(current_version_id.as_str())
        );
        assert_eq!(restored_version.graph_path, base_version.graph_path);
        assert_eq!(
            body["workflowGraph"]["nodes"]["video"]["params"]["duration_sec"],
            5
        );
        assert!(
            body["history"]
                .as_array()
                .expect("history")
                .iter()
                .any(|item| item["summary"] == "restore graph" && item["source"] == "restore")
        );
    }

    #[tokio::test]
    async fn restore_workspace_version_creates_new_current_version_from_target_graph() {
        let (state, workspace_id, base_version_id, current_version_id, _dir) =
            state_with_two_versions().await;

        let body = restore_workspace_version(
            Path((workspace_id.clone(), base_version_id.clone())),
            State(state.clone()),
        )
        .await
        .expect("restore version")
        .0;
        let restored_version_id = body["workspace"]["versionId"].as_str().expect("version id");
        let restored_version = state
            .store
            .version(restored_version_id)
            .await
            .expect("restored version");

        assert_ne!(restored_version_id, base_version_id);
        assert_eq!(restored_version.source, "restore");
        assert_eq!(
            restored_version.parent_id.as_deref(),
            Some(current_version_id.as_str())
        );
        assert!(restored_version.label.contains("Restore"));
        assert_eq!(
            body["workflowGraph"]["nodes"]["video"]["params"]["duration_sec"],
            5
        );

        let reloaded = crate::workspace_state::workspace_state(
            Path(workspace_id.clone()),
            State(state.clone()),
        )
        .await
        .expect("reload workspace state")
        .0;
        assert_eq!(reloaded["workspace"]["versionId"], restored_version_id);
        assert_eq!(
            reloaded["workflowGraph"]["nodes"]["video"]["params"]["duration_sec"],
            5
        );
    }

    #[tokio::test]
    async fn restore_rejects_corrupt_target_verified_read_without_side_effects() {
        for payload in [
            StoredTargetGraph::Missing,
            StoredTargetGraph::InvalidStoredHash,
            StoredTargetGraph::HashMismatch,
            StoredTargetGraph::InvalidJson,
        ] {
            let (state, workspace_id, target_id, current_id, _dir) =
                state_with_target_payload(payload).await;
            let version_ids_before: Vec<_> = state
                .store
                .versions_for_workspace(&workspace_id)
                .await
                .expect("versions before")
                .into_iter()
                .map(|version| version.id)
                .collect();

            let export_error =
                export_workflow_version(Path(target_id.clone()), State(state.clone()))
                    .await
                    .expect_err("corrupt target must not be exported");
            let restore_error = restore_workspace_version(
                Path((workspace_id.clone(), target_id)),
                State(state.clone()),
            )
            .await
            .expect_err("corrupt target must not be restored");

            assert_eq!(
                export_error.status,
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
            );
            assert_eq!(
                restore_error.status,
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
            );
            assert!([&export_error, &restore_error].iter().all(|error| {
                !error
                    .message
                    .contains(state.data_dir.to_string_lossy().as_ref())
            }));
            assert_eq!(
                state
                    .store
                    .versions_for_workspace(&workspace_id)
                    .await
                    .expect("versions after")
                    .into_iter()
                    .map(|version| version.id)
                    .collect::<Vec<_>>(),
                version_ids_before
            );
            assert_eq!(
                state
                    .store
                    .workspace(&workspace_id)
                    .await
                    .expect("workspace")
                    .cur_version_id
                    .as_deref(),
                Some(current_id.as_str())
            );
            assert!(
                state
                    .store
                    .workspace_messages(&workspace_id)
                    .await
                    .expect("messages")
                    .is_empty()
            );
            assert!(
                state
                    .store
                    .latest_workspace_run(&workspace_id)
                    .await
                    .expect("latest run")
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn undo_workspace_version_rejects_initial_version() {
        let (state, version_id, _dir) = state_with_graph(current_graph()).await;
        let workspace_id = state
            .store
            .version(&version_id)
            .await
            .expect("version")
            .workspace_id;

        let err = undo_workspace_version(Path(workspace_id), State(state))
            .await
            .expect_err("initial version cannot be undone");

        assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
    }

    #[test]
    fn export_safety_rejects_camel_case_and_header_keys() {
        for key in [
            "accessToken",
            "privateKey",
            "authHeader",
            "requestHeaders",
            "providerRequestId",
            "runId",
            "sessionId",
            "traceId",
            "runtimeOnlyMetadata",
        ] {
            assert!(is_non_exportable_key(key), "{key} should be non-exportable");
        }
    }

    #[test]
    fn export_safety_rejects_generic_absolute_paths() {
        for value in [
            "/etc/helixflow/config.json",
            "/var/tmp/render.png",
            "/opt/helixflow/model.safetensors",
            "/root/.config/provider.json",
            "/mnt/workflow/output.mp4",
            "~/workflow/output.mp4",
            "C:\\Users\\alice\\workflow.json",
            "\\Users\\alice\\workflow.json",
        ] {
            assert!(
                looks_like_local_absolute_path(value),
                "{value} should be treated as a local absolute path"
            );
        }

        assert!(!looks_like_local_absolute_path(
            "workspace://outputs/run_1/video.mp4"
        ));
        assert!(!looks_like_local_absolute_path(
            "https://example.com/workflow.json"
        ));
    }

    async fn state_with_current_graph_and_pending_proposal() -> (AppState, String, tempfile::TempDir)
    {
        let (state, version_id, dir) = state_with_graph(current_graph()).await;
        let workspace_id = state
            .store
            .version(&version_id)
            .await
            .expect("version")
            .workspace_id;
        let proposal_dir = PathBuf::from("workspaces")
            .join(&workspace_id)
            .join("proposals")
            .join("export-test");
        tokio::fs::create_dir_all(state.data_dir.join(&proposal_dir))
            .await
            .expect("proposal dir");
        let ops_path = proposal_dir.join("ops.json");
        let preview_path = proposal_dir.join("preview.json");
        tokio::fs::write(state.data_dir.join(&ops_path), b"[]")
            .await
            .expect("write ops");
        tokio::fs::write(
            state.data_dir.join(&preview_path),
            serde_json::to_vec_pretty(&preview_graph()).expect("preview json"),
        )
        .await
        .expect("write preview");
        let ops_path_string = ops_path.to_string_lossy().into_owned();
        let preview_path_string = preview_path.to_string_lossy().into_owned();
        state
            .store
            .create_proposal(NewProposal {
                workspace_id: &workspace_id,
                base_version_id: &version_id,
                kind: "modify",
                title: "Shorter clip",
                summary: "Set duration to four seconds.",
                ops_path: &ops_path_string,
                preview_graph_path: Some(&preview_path_string),
                message_id: None,
            })
            .await
            .expect("create proposal");
        (state, version_id, dir)
    }

    async fn state_with_two_versions() -> (AppState, String, String, String, tempfile::TempDir) {
        state_with_target_payload(StoredTargetGraph::Valid).await
    }

    #[derive(Clone, Copy)]
    enum StoredTargetGraph {
        Valid,
        Missing,
        InvalidStoredHash,
        HashMismatch,
        InvalidJson,
    }

    async fn state_with_target_payload(
        payload: StoredTargetGraph,
    ) -> (AppState, String, String, String, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store
            .create_workspace("Version route workspace")
            .await
            .expect("create workspace");
        let graph_dir = PathBuf::from("workspaces")
            .join(&workspace.id)
            .join("graphs");
        tokio::fs::create_dir_all(data_dir.join(&graph_dir))
            .await
            .expect("create graph dir");
        let base_path = graph_dir.join("base.json");
        let current_path = graph_dir.join("current.json");
        let canonical_base = serde_json::to_vec_pretty(&current_graph()).expect("base graph json");
        let (base_bytes, base_hash) = match payload {
            StoredTargetGraph::Valid => (Some(canonical_base.clone()), graph_hash(&canonical_base)),
            StoredTargetGraph::Missing => (None, graph_hash(&canonical_base)),
            StoredTargetGraph::InvalidStoredHash => {
                (Some(canonical_base), "sha256:not-canonical".to_owned())
            }
            StoredTargetGraph::HashMismatch => (Some(b"{}".to_vec()), graph_hash(&canonical_base)),
            StoredTargetGraph::InvalidJson => {
                let bytes = b"invalid restore graph json".to_vec();
                let hash = graph_hash(&bytes);
                (Some(bytes), hash)
            }
        };
        if let Some(base_bytes) = base_bytes {
            tokio::fs::write(data_dir.join(&base_path), base_bytes)
                .await
                .expect("write base graph");
        }
        let current_bytes =
            serde_json::to_vec_pretty(&preview_graph()).expect("current graph json");
        let current_hash = graph_hash(&current_bytes);
        tokio::fs::write(data_dir.join(&current_path), current_bytes)
            .await
            .expect("write current graph");
        let base_path_string = base_path.to_string_lossy().into_owned();
        let current_path_string = current_path.to_string_lossy().into_owned();
        let base = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Base graph",
                source: VersionSource::Manual,
                graph_path: &base_path_string,
                graph_hash: &base_hash,
                parent_id: None,
                semantics_json: None,
            })
            .await
            .expect("create base version");
        let current = store
            .create_version_after(
                NewVersion {
                    workspace_id: &workspace.id,
                    label: "Shorter clip",
                    source: VersionSource::Proposal,
                    graph_path: &current_path_string,
                    graph_hash: &current_hash,
                    parent_id: Some(&base.id),
                    semantics_json: None,
                },
                &base.id,
            )
            .await
            .expect("create current version");
        let state = AppState::with_store_agent(
            EventBus::new(16),
            store,
            data_dir.clone(),
            Arc::new(NoopWorkbenchAgent),
            data_dir.join("sessions"),
        );
        (state, workspace.id, base.id, current.id, dir)
    }

    async fn state_with_graph(graph: WorkflowGraph) -> (AppState, String, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store
            .create_workspace("Export route workspace")
            .await
            .expect("create workspace");
        let graph_path = PathBuf::from("workspaces")
            .join(&workspace.id)
            .join("graphs")
            .join("current.json");
        tokio::fs::create_dir_all(data_dir.join(graph_path.parent().expect("graph parent")))
            .await
            .expect("create graph dir");
        let graph_bytes = serde_json::to_vec_pretty(&graph).expect("graph json");
        let stored_graph_hash = graph_hash(&graph_bytes);
        tokio::fs::write(data_dir.join(&graph_path), graph_bytes)
            .await
            .expect("write graph");
        let graph_path_string = graph_path.to_string_lossy().into_owned();
        let version = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Current graph",
                source: VersionSource::Manual,
                graph_path: &graph_path_string,
                graph_hash: &stored_graph_hash,
                parent_id: None,
                semantics_json: None,
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
        (state, version.id, dir)
    }

    fn current_graph() -> WorkflowGraph {
        workflow_graph(json!({ "prompt": "make a product clip", "duration_sec": 5 }))
    }

    fn preview_graph() -> WorkflowGraph {
        workflow_graph(json!({ "prompt": "make a product clip", "duration_sec": 4 }))
    }

    fn unsafe_graph() -> WorkflowGraph {
        workflow_graph(json!({ "api_key": "sk_live_should_not_leak" }))
    }

    fn workflow_graph(params: serde_json::Value) -> WorkflowGraph {
        WorkflowGraph {
            schema_version: 1,
            nodes: BTreeMap::from([
                (
                    "input".to_owned(),
                    GraphNode {
                        node_type: "input.text".to_owned(),
                        title: "Text".to_owned(),
                        params: json!({ "text": "launch teaser" }),
                        pos: [0.0, 0.0],
                        size: None,
                        semantics: None,
                    },
                ),
                (
                    "video".to_owned(),
                    GraphNode {
                        node_type: "video.text_to_video".to_owned(),
                        title: "Video render".to_owned(),
                        params,
                        pos: [240.0, 0.0],
                        size: None,
                        semantics: None,
                    },
                ),
            ]),
            edges: vec![GraphEdge {
                from: ["input".to_owned(), "text".to_owned()],
                to: ["video".to_owned(), "prompt".to_owned()],
                edge_type: "text".to_owned(),
            }],
            catalog_revision: None,
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
