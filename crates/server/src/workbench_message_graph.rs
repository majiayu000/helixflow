use helixflow_graph::WorkflowGraph;
use helixflow_store::VersionRecord;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::version_file_consistency::read_version_graph;

pub(crate) struct VerifiedMessageGraph {
    pub(crate) version: VersionRecord,
    pub(crate) graph: WorkflowGraph,
}

pub(crate) async fn verified_message_graph(
    state: &AppState,
    workspace_id: &str,
    base_version_id: &str,
    client_graph: &WorkflowGraph,
) -> Result<VerifiedMessageGraph, ApiError> {
    let workspace = state
        .store
        .workspace(workspace_id)
        .await
        .map_err(ApiError::store)?;
    if workspace.cur_version_id.as_deref() != Some(base_version_id) {
        return Err(ApiError::conflict(
            "message base version is not the workspace current version",
        ));
    }
    let version = state
        .store
        .version(base_version_id)
        .await
        .map_err(ApiError::store)?;
    ensure_version_workspace(&version, workspace_id)?;
    let graph = read_version_graph(&state.data_dir, &version)
        .await
        .map_err(|error| ApiError::server_error(error.to_string()))?;
    if &graph != client_graph {
        return Err(ApiError::conflict(
            "message graph does not match the verified workspace version",
        ));
    }
    Ok(VerifiedMessageGraph { version, graph })
}

fn ensure_version_workspace(version: &VersionRecord, workspace_id: &str) -> Result<(), ApiError> {
    if version.workspace_id == workspace_id {
        return Ok(());
    }
    Err(ApiError::conflict(
        "message base version does not belong to the workspace",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_read_rejects_foreign_version_record() {
        let version = VersionRecord {
            id: "ver_foreign".to_owned(),
            workspace_id: "ws_foreign".to_owned(),
            idx: 1,
            label: "Foreign".to_owned(),
            source: "manual".to_owned(),
            graph_path: "graphs/foreign.json".to_owned(),
            graph_hash: format!("sha256:{}", "0".repeat(64)),
            parent_id: None,
            semantics_json: None,
            created_at: "now".to_owned(),
        };

        let error = ensure_version_workspace(&version, "ws_requested")
            .expect_err("foreign version must be rejected");

        assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
        assert!(!error.message.contains("ws_foreign"));
        assert!(!error.message.contains("ws_requested"));
    }
}
