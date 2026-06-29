use std::path::PathBuf;

use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_graph::{GraphService, PreparedProposal, ProposalOp, ProposalState};
use helixflow_registry::NodeRegistry;
use helixflow_store::{
    ApplyProposalVersionRecord, NewMessage, NewVersion, ProposalRecord, ProposalResolutionState,
    ResolveProposal, VersionSource,
};
use serde_json::{Value, json};

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::{read_graph_file, read_json_file, write_json_file};
use crate::workbench_payload::proposal_kind_from_str;
use crate::workspace_state::workspace_state_value;

pub(crate) async fn apply_workspace_proposal(
    Path((workspace_id, proposal_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<Value>, ApiError> {
    let proposal = proposal_for_workspace(&state, &workspace_id, &proposal_id).await?;
    ensure_pending_record(&proposal)?;
    let workspace = state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    let current_version_id = workspace.cur_version_id.as_deref().ok_or_else(|| {
        ApiError::bad_request(format!(
            "workspace `{workspace_id}` has no current version for proposal apply"
        ))
    })?;
    let current_version = state
        .store
        .version(current_version_id)
        .await
        .map_err(ApiError::store)?;
    let current_graph = read_graph_file(&state.data_dir, &current_version.graph_path).await?;
    let prepared = prepared_proposal(&state, &proposal).await?;
    let applied_graph = GraphService::new(NodeRegistry::builtin())
        .apply_proposal(&current_graph, current_version_id, &prepared)
        .map_err(|err| ApiError::bad_request(err.to_string()))?;
    let graph_path = applied_graph_path(&proposal);
    let graph_hash = write_json_file(
        &state.data_dir,
        &graph_path,
        &applied_graph,
        "write applied proposal graph",
    )
    .await?;
    let graph_path_string = graph_path.to_string_lossy().into_owned();
    let version_label = format!("Apply proposal: {}", proposal.title);
    let version = state
        .store
        .create_version_after_applying_proposal(ApplyProposalVersionRecord {
            proposal_id: &proposal.id,
            expected_current_version_id: current_version_id,
            version: NewVersion {
                workspace_id: &workspace_id,
                label: &version_label,
                source: VersionSource::Proposal,
                graph_path: &graph_path_string,
                graph_hash: &graph_hash,
                parent_id: Some(&proposal.base_version_id),
            },
        })
        .await
        .map_err(ApiError::store)?;
    state
        .store
        .create_message(NewMessage {
            workspace_id: &workspace_id,
            role: "agent",
            kind: "proposal_applied",
            text: Some(&format!("Applied proposal `{}`.", proposal.title)),
            ref_id: Some(&proposal.id),
            attachment_ids_json: Some(&json!({ "versionId": version.id }).to_string()),
        })
        .await
        .map_err(ApiError::store)?;

    Ok(Json(workspace_state_value(&state, &workspace_id).await?))
}

pub(crate) async fn dismiss_workspace_proposal(
    Path((workspace_id, proposal_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<Value>, ApiError> {
    let proposal = proposal_for_workspace(&state, &workspace_id, &proposal_id).await?;
    ensure_pending_record(&proposal)?;
    state
        .store
        .resolve_proposal(ResolveProposal {
            proposal_id: &proposal.id,
            workspace_id: &workspace_id,
            state: ProposalResolutionState::Dismissed,
            result_version_id: None,
        })
        .await
        .map_err(ApiError::store)?;
    state
        .store
        .create_message(NewMessage {
            workspace_id: &workspace_id,
            role: "agent",
            kind: "proposal_dismissed",
            text: Some(&format!("Dismissed proposal `{}`.", proposal.title)),
            ref_id: Some(&proposal.id),
            attachment_ids_json: None,
        })
        .await
        .map_err(ApiError::store)?;

    Ok(Json(workspace_state_value(&state, &workspace_id).await?))
}

async fn proposal_for_workspace(
    state: &AppState,
    workspace_id: &str,
    proposal_id: &str,
) -> Result<ProposalRecord, ApiError> {
    let proposal = match state.store.proposal(proposal_id).await {
        Ok(proposal) => proposal,
        Err(err) if err.is_not_found() => {
            return Err(ApiError::not_found("proposal was not found"));
        }
        Err(err) => return Err(ApiError::store(err)),
    };
    if proposal.workspace_id != workspace_id {
        return Err(ApiError::not_found(
            "proposal was not found in this workspace",
        ));
    }
    Ok(proposal)
}

fn ensure_pending_record(proposal: &ProposalRecord) -> Result<(), ApiError> {
    if proposal.state != "pending" {
        return Err(ApiError::bad_request(format!(
            "proposal `{}` is `{}` and cannot be changed",
            proposal.id, proposal.state
        )));
    }
    Ok(())
}

async fn prepared_proposal(
    state: &AppState,
    proposal: &ProposalRecord,
) -> Result<PreparedProposal, ApiError> {
    let preview_path = proposal.preview_graph_path.as_deref().ok_or_else(|| {
        ApiError::server_error(format!(
            "pending proposal `{}` has no preview graph",
            proposal.id
        ))
    })?;
    let ops: Vec<ProposalOp> =
        read_json_file(&state.data_dir, &proposal.ops_path, "read proposal ops").await?;
    let preview_graph = read_graph_file(&state.data_dir, preview_path).await?;

    Ok(PreparedProposal {
        base_version_id: proposal.base_version_id.clone(),
        kind: proposal_kind_from_str(&proposal.kind).map_err(ApiError::server_error)?,
        title: proposal.title.clone(),
        summary: proposal.summary.clone(),
        ops,
        diff_summary: Vec::new(),
        preview_graph,
        state: ProposalState::Pending,
        message_id: proposal.message_id.clone(),
    })
}

fn applied_graph_path(proposal: &ProposalRecord) -> PathBuf {
    PathBuf::from("workspaces")
        .join(&proposal.workspace_id)
        .join("graphs")
        .join(format!("{}.json", proposal.id))
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
    use helixflow_graph::{
        GraphEdge, GraphNode, GraphService, ProposalDraft, ProposalKind, ProposalOp, WorkflowGraph,
    };
    use helixflow_registry::NodeRegistry;
    use helixflow_run::EventBus;
    use helixflow_store::{NewProposal, NewVersion, Store, VersionSource};
    use serde_json::json;

    use super::*;
    use crate::app_state::{AppState, WorkbenchAgent};

    #[tokio::test]
    async fn apply_workspace_proposal_creates_version_and_clears_pending_proposal() {
        let (state, workspace_id, base_version_id, proposal_id, _dir) =
            state_with_pending_proposal().await;

        let body = apply_workspace_proposal(
            Path((workspace_id.clone(), proposal_id.clone())),
            State(state.clone()),
        )
        .await
        .expect("apply proposal")
        .0;

        let proposal = state.store.proposal(&proposal_id).await.expect("proposal");
        assert_eq!(proposal.state, "applied");
        assert_ne!(body["workspace"]["versionId"], base_version_id);
        assert!(body["pendingProposal"].is_null());
        assert_eq!(
            body["workflowGraph"]["nodes"]["video"]["params"]["duration_sec"],
            4
        );
        assert!(
            state
                .store
                .workspace_messages(&workspace_id)
                .await
                .expect("messages")
                .iter()
                .any(|message| message.kind == "proposal_applied")
        );
    }

    #[tokio::test]
    async fn dismiss_workspace_proposal_leaves_current_version_unchanged() {
        let (state, workspace_id, base_version_id, proposal_id, _dir) =
            state_with_pending_proposal().await;

        let body = dismiss_workspace_proposal(
            Path((workspace_id.clone(), proposal_id.clone())),
            State(state.clone()),
        )
        .await
        .expect("dismiss proposal")
        .0;

        let proposal = state.store.proposal(&proposal_id).await.expect("proposal");
        assert_eq!(proposal.state, "dismissed");
        assert_eq!(body["workspace"]["versionId"], base_version_id);
        assert!(body["pendingProposal"].is_null());
        assert_eq!(
            body["workflowGraph"]["nodes"]["video"]["params"]["duration_sec"],
            5
        );
    }

    async fn state_with_pending_proposal() -> (AppState, String, String, String, tempfile::TempDir)
    {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store
            .create_workspace("Proposal route workspace")
            .await
            .expect("create workspace");
        let graph_path = PathBuf::from("workspaces")
            .join(&workspace.id)
            .join("graphs")
            .join("base.json");
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
                label: "Base graph",
                source: VersionSource::Manual,
                graph_path: &graph_path_string,
                graph_hash: "sha256:base",
                parent_id: None,
            })
            .await
            .expect("create version");
        let draft = ProposalDraft {
            base_version_id: version.id.clone(),
            kind: ProposalKind::Modify,
            title: "Shorter clip".to_owned(),
            summary: "Set duration to four seconds.".to_owned(),
            ops: vec![ProposalOp::SetParam {
                id: "video".to_owned(),
                key: "duration_sec".to_owned(),
                prev: Some(json!(5)),
                value: json!(4),
            }],
            message_id: None,
        };
        let prepared = GraphService::new(NodeRegistry::builtin())
            .preview_proposal(&sample_graph(), &version.id, draft)
            .expect("preview proposal");
        let proposal_dir = PathBuf::from("workspaces")
            .join(&workspace.id)
            .join("proposals")
            .join("test-session");
        tokio::fs::create_dir_all(data_dir.join(&proposal_dir))
            .await
            .expect("create proposal dir");
        let ops_path = proposal_dir.join("ops.json");
        let preview_path = proposal_dir.join("preview.json");
        tokio::fs::write(
            data_dir.join(&ops_path),
            serde_json::to_vec_pretty(&prepared.ops).expect("ops json"),
        )
        .await
        .expect("write ops");
        tokio::fs::write(
            data_dir.join(&preview_path),
            serde_json::to_vec_pretty(&prepared.preview_graph).expect("preview json"),
        )
        .await
        .expect("write preview");
        let ops_path_string = ops_path.to_string_lossy().into_owned();
        let preview_path_string = preview_path.to_string_lossy().into_owned();
        let proposal = store
            .create_proposal(NewProposal {
                workspace_id: &workspace.id,
                base_version_id: &version.id,
                kind: "modify",
                title: "Shorter clip",
                summary: "Set duration to four seconds.",
                ops_path: &ops_path_string,
                preview_graph_path: Some(&preview_path_string),
                message_id: None,
            })
            .await
            .expect("create proposal");
        let state = AppState::with_store_agent(
            EventBus::new(16),
            store,
            data_dir.clone(),
            Arc::new(NoopWorkbenchAgent),
            data_dir.join("sessions"),
        );
        (state, workspace.id, version.id, proposal.id, dir)
    }

    fn sample_graph() -> WorkflowGraph {
        WorkflowGraph {
            schema_version: 1,
            nodes: BTreeMap::from([
                (
                    "input".to_owned(),
                    GraphNode {
                        node_type: "input.text".to_owned(),
                        title: "Text".to_owned(),
                        params: json!({ "text": "make a product clip" }),
                        pos: [0.0, 0.0],
                    },
                ),
                (
                    "writer".to_owned(),
                    GraphNode {
                        node_type: "llm.prompt_writer".to_owned(),
                        title: "Prompt".to_owned(),
                        params: json!({ "style": "product" }),
                        pos: [220.0, 0.0],
                    },
                ),
                (
                    "video".to_owned(),
                    GraphNode {
                        node_type: "video.mock.text_to_video".to_owned(),
                        title: "Video".to_owned(),
                        params: json!({
                            "prompt": "clean product shot",
                            "duration_sec": 5,
                            "aspect_ratio": "9:16"
                        }),
                        pos: [440.0, 0.0],
                    },
                ),
            ]),
            edges: vec![
                GraphEdge {
                    from: ["input".to_owned(), "text".to_owned()],
                    to: ["writer".to_owned(), "text".to_owned()],
                    edge_type: "text".to_owned(),
                },
                GraphEdge {
                    from: ["writer".to_owned(), "prompt".to_owned()],
                    to: ["video".to_owned(), "prompt".to_owned()],
                    edge_type: "text".to_owned(),
                },
            ],
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
