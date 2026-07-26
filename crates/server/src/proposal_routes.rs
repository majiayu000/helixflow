#[cfg(test)]
use std::path::PathBuf;

use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_graph::{GraphError, GraphService, PreparedProposal, ProposalOp, ProposalState};
use helixflow_registry::NodeRegistry;
use helixflow_store::{
    ApplyProposalVersionRecord, NewMessage, NewVersion, ProposalRecord, ProposalResolutionState,
    ResolveProposal, StoreError, VersionSource,
};
use serde_json::Value;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::{read_graph_file, read_json_file};
use crate::version_file_consistency::{
    CandidateKind, VersionFileCandidate, VersionFileConsistencyError, read_version_graph,
};
use crate::version_migration_routes::derive_semantics_json;
use crate::workbench_payload::proposal_kind_from_str;
use crate::workspace_state::workspace_state_value;

#[cfg(test)]
#[derive(Clone)]
struct ApplyProposalCommitHook {
    published: std::sync::Arc<tokio::sync::Barrier>,
    release: std::sync::Arc<tokio::sync::Barrier>,
}

#[cfg(test)]
impl ApplyProposalCommitHook {
    async fn after_publish(&self) {
        self.published.wait().await;
        self.release.wait().await;
    }
}

pub(crate) async fn apply_workspace_proposal(
    path: Path<(String, String)>,
    state: State<AppState>,
) -> Result<Json<Value>, ApiError> {
    #[cfg(test)]
    {
        apply_workspace_proposal_inner(path, state, None).await
    }
    #[cfg(not(test))]
    {
        apply_workspace_proposal_inner(path, state).await
    }
}

async fn apply_workspace_proposal_inner(
    Path((workspace_id, proposal_id)): Path<(String, String)>,
    State(state): State<AppState>,
    #[cfg(test)] commit_hook: Option<&ApplyProposalCommitHook>,
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
    let current_graph = read_version_graph(&state.data_dir, &current_version)
        .await
        .map_err(candidate_error)?;
    let prepared = prepared_proposal(&state, &proposal).await?;
    let applied_graph = GraphService::new(NodeRegistry::builtin())
        .apply_proposal(&current_graph, current_version_id, &prepared)
        .map_err(graph_apply_error)?;
    let mut candidate = VersionFileCandidate::from_graph(
        &workspace_id,
        CandidateKind::ProposalApplied,
        &applied_graph,
    )
    .map_err(candidate_error)?;
    let graph_path = candidate
        .relative_path_text()
        .map_err(candidate_error)?
        .to_owned();
    let graph_hash = candidate.graph_hash().to_owned();
    let semantics_json = derive_semantics_json(&current_version, &current_graph, &applied_graph)?;
    candidate
        .publish(&state.data_dir)
        .map_err(candidate_error)?;
    #[cfg(test)]
    if let Some(hook) = commit_hook {
        hook.after_publish().await;
    }
    let version_label = format!("Apply proposal: {}", proposal.title);
    let message_text = format!("Applied proposal `{}`.", proposal.title);
    let result = state
        .store
        .create_version_after_applying_proposal(ApplyProposalVersionRecord {
            proposal_id: &proposal.id,
            expected_current_version_id: current_version_id,
            version: NewVersion {
                workspace_id: &workspace_id,
                label: &version_label,
                source: VersionSource::Proposal,
                graph_path: &graph_path,
                graph_hash: &graph_hash,
                parent_id: Some(&proposal.base_version_id),
                semantics_json: semantics_json.as_deref(),
            },
            message_text: &message_text,
        })
        .await;
    match result {
        Ok(_) => candidate.mark_committed().map_err(candidate_error)?,
        Err(store_error) => {
            return Err(cleanup_applied_candidate(&state, &mut candidate, store_error).await);
        }
    }

    Ok(Json(workspace_state_value(&state, &workspace_id).await?))
}

async fn cleanup_applied_candidate(
    state: &AppState,
    candidate: &mut VersionFileCandidate,
    store_error: StoreError,
) -> ApiError {
    match candidate.cleanup_after_store_error(&state.store).await {
        Ok(_) => match store_error {
            StoreError::VersionConflict { .. }
            | StoreError::VersionParentMismatch { .. }
            | StoreError::ProposalBaseMismatch { .. }
            | StoreError::ProposalStateConflict { .. } => ApiError::conflict(
                "workspace or proposal changed while applying; refresh and retry",
            ),
            error => ApiError::store(error),
        },
        Err(cleanup_error) => ApiError::server_error(format!(
            "proposal commit failed and candidate cleanup was deferred: {cleanup_error}"
        )),
    }
}

fn candidate_error(error: VersionFileConsistencyError) -> ApiError {
    ApiError::server_error(error.to_string())
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

pub(crate) fn graph_apply_error(err: GraphError) -> ApiError {
    match err {
        GraphError::ProposalSuperseded { .. } => ApiError::conflict(err.to_string()),
        _ => ApiError::bad_request(err.to_string()),
    }
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
    use crate::graph_files::graph_hash;

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
        let version_id = body["workspace"]["versionId"]
            .as_str()
            .expect("applied version id");
        assert_ne!(version_id, base_version_id);
        assert_eq!(proposal.result_version_id.as_deref(), Some(version_id));
        assert!(body["pendingProposal"].is_null());
        assert_eq!(
            body["workflowGraph"]["nodes"]["video"]["params"]["duration_sec"],
            4
        );
        let version = state
            .store
            .version(version_id)
            .await
            .expect("applied version");
        let bytes = tokio::fs::read(state.data_dir.join(&version.graph_path))
            .await
            .expect("applied bytes");
        assert_eq!(version.graph_hash, graph_hash(&bytes));
        let verified_graph = read_version_graph(&state.data_dir, &version)
            .await
            .expect("verified applied graph");
        assert_eq!(
            serde_json::to_value(verified_graph).expect("serialize verified graph"),
            body["workflowGraph"]
        );
        let messages = state
            .store
            .workspace_messages(&workspace_id)
            .await
            .expect("messages");
        let applied_messages: Vec<_> = messages
            .iter()
            .filter(|message| message.kind == "proposal_applied")
            .collect();
        assert_eq!(applied_messages.len(), 1);
        assert_eq!(
            applied_messages[0].ref_id.as_deref(),
            Some(proposal_id.as_str())
        );
        assert_eq!(
            applied_messages[0]
                .attachment_ids_json
                .as_deref()
                .and_then(|value| serde_json::from_str::<Value>(value).ok())
                .and_then(|value| value["versionId"].as_str().map(str::to_owned)),
            Some(version_id.to_owned())
        );
    }

    #[tokio::test]
    async fn apply_workspace_proposal_store_failure_cleans_unreferenced_candidate() {
        let (state, workspace_id, base_version_id, proposal_id, _dir) =
            state_with_pending_proposal().await;
        let hook = ApplyProposalCommitHook {
            published: Arc::new(tokio::sync::Barrier::new(2)),
            release: Arc::new(tokio::sync::Barrier::new(2)),
        };
        let task_state = state.clone();
        let task_hook = hook.clone();
        let task_workspace_id = workspace_id.clone();
        let task_proposal_id = proposal_id.clone();
        let task = tokio::spawn(async move {
            apply_workspace_proposal_inner(
                Path((task_workspace_id, task_proposal_id)),
                State(task_state),
                Some(&task_hook),
            )
            .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), hook.published.wait())
            .await
            .expect("candidate publish checkpoint");
        state
            .store
            .resolve_proposal(ResolveProposal {
                proposal_id: &proposal_id,
                workspace_id: &workspace_id,
                state: ProposalResolutionState::Dismissed,
                result_version_id: None,
            })
            .await
            .expect("dismiss after candidate publish");
        hook.release.wait().await;
        let error = task
            .await
            .expect("proposal apply task")
            .expect_err("Store fault must fail apply");

        assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
        assert_eq!(
            state
                .store
                .workspace(&workspace_id)
                .await
                .expect("workspace")
                .cur_version_id
                .as_deref(),
            Some(base_version_id.as_str())
        );
        assert_eq!(
            state
                .store
                .versions_for_workspace(&workspace_id)
                .await
                .expect("versions")
                .len(),
            1
        );
        assert!(
            state
                .store
                .workspace_messages(&workspace_id)
                .await
                .expect("messages")
                .is_empty()
        );
        assert_eq!(
            state
                .store
                .proposal(&proposal_id)
                .await
                .expect("proposal")
                .state,
            "dismissed"
        );
        let entries = graph_entry_names(&state, &workspace_id).await;
        assert_eq!(entries, vec!["base.json"]);
        assert_no_candidate_temps(&entries);
    }

    #[tokio::test]
    async fn concurrent_same_base_manual_proposals_have_one_winner_and_clean_loser() {
        let (state, workspace_id, base_version_id, first_id, _dir) =
            state_with_pending_proposal().await;
        let first = state
            .store
            .proposal(&first_id)
            .await
            .expect("first proposal");
        let second = state
            .store
            .create_proposal(NewProposal {
                workspace_id: &workspace_id,
                base_version_id: &base_version_id,
                kind: &first.kind,
                title: "Concurrent proposal",
                summary: &first.summary,
                ops_path: &first.ops_path,
                preview_graph_path: first.preview_graph_path.as_deref(),
                message_id: None,
            })
            .await
            .expect("second proposal");
        let hook = ApplyProposalCommitHook {
            published: Arc::new(tokio::sync::Barrier::new(3)),
            release: Arc::new(tokio::sync::Barrier::new(3)),
        };
        let first_task = spawn_proposal_apply(
            state.clone(),
            workspace_id.clone(),
            first_id.clone(),
            hook.clone(),
        );
        let second_task = spawn_proposal_apply(
            state.clone(),
            workspace_id.clone(),
            second.id.clone(),
            hook.clone(),
        );
        hook.published.wait().await;
        hook.release.wait().await;
        let results = [
            first_task.await.expect("first apply joined"),
            second_task.await.expect("second apply joined"),
        ];

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let loser = results
            .iter()
            .find_map(|result| result.as_ref().err())
            .expect("one apply conflicts");
        assert_eq!(loser.status, axum::http::StatusCode::CONFLICT);
        let versions = state
            .store
            .versions_for_workspace(&workspace_id)
            .await
            .expect("versions");
        assert_eq!(versions.len(), 2, "loser must not leave a version");
        let winner = &versions[1];
        assert_eq!(winner.parent_id.as_deref(), Some(base_version_id.as_str()));
        let workspace = state.store.workspace(&workspace_id).await.unwrap();
        assert_eq!(workspace.cur_version_id, Some(winner.id.clone()));
        let bytes = tokio::fs::read(state.data_dir.join(&winner.graph_path))
            .await
            .expect("winner bytes");
        assert_eq!(winner.graph_hash, graph_hash(&bytes));
        read_version_graph(&state.data_dir, winner)
            .await
            .expect("verified winner graph");
        let proposals = [
            state.store.proposal(&first_id).await.unwrap(),
            state.store.proposal(&second.id).await.unwrap(),
        ];
        assert!(
            matches!(
                (proposals[0].state.as_str(), proposals[1].state.as_str()),
                ("applied", "pending") | ("pending", "applied")
            ),
            "one proposal must apply and one must remain pending"
        );
        let applied = proposals
            .iter()
            .find(|value| value.state == "applied")
            .unwrap();
        assert_eq!(
            applied.result_version_id.as_deref(),
            Some(winner.id.as_str())
        );
        let messages = state.store.workspace_messages(&workspace_id).await.unwrap();
        assert_eq!(messages.len(), 1, "loser must not leave a message");
        assert_eq!(messages[0].ref_id, Some(applied.id.clone()));
        let entries = graph_entry_names(&state, &workspace_id).await;
        assert_eq!(entries.len(), 2, "loser candidate must be removed");
        assert_no_candidate_temps(&entries);
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

    #[tokio::test]
    async fn apply_workspace_proposal_rejects_stale_base_version_with_conflict() {
        let (state, workspace_id, base_version_id, proposal_id, _dir) =
            state_with_pending_proposal().await;
        let next_graph_path = PathBuf::from("workspaces")
            .join(&workspace_id)
            .join("graphs")
            .join("next.json");
        let next_graph_bytes = serde_json::to_vec_pretty(&sample_graph()).expect("next graph json");
        tokio::fs::write(state.data_dir.join(&next_graph_path), &next_graph_bytes)
            .await
            .expect("write next graph");
        let next_graph_path_string = next_graph_path.to_string_lossy().into_owned();
        state
            .store
            .create_version_after(
                NewVersion {
                    workspace_id: &workspace_id,
                    label: "Manual change",
                    source: VersionSource::Manual,
                    graph_path: &next_graph_path_string,
                    graph_hash: &graph_hash(&next_graph_bytes),
                    parent_id: Some(&base_version_id),
                    semantics_json: None,
                },
                &base_version_id,
            )
            .await
            .expect("advance current version");

        let err = apply_workspace_proposal(
            Path((workspace_id.clone(), proposal_id.clone())),
            State(state.clone()),
        )
        .await
        .expect_err("stale proposal should conflict");

        assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
        let proposal = state.store.proposal(&proposal_id).await.expect("proposal");
        assert_eq!(proposal.state, "pending");
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
        let graph_bytes = serde_json::to_vec_pretty(&sample_graph()).expect("graph json");
        tokio::fs::write(data_dir.join(&graph_path), &graph_bytes)
            .await
            .expect("write graph");
        let graph_path_string = graph_path.to_string_lossy().into_owned();
        let version = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Base graph",
                source: VersionSource::Manual,
                graph_path: &graph_path_string,
                graph_hash: &graph_hash(&graph_bytes),
                parent_id: None,
                semantics_json: None,
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

    fn spawn_proposal_apply(
        state: AppState,
        workspace_id: String,
        proposal_id: String,
        hook: ApplyProposalCommitHook,
    ) -> tokio::task::JoinHandle<Result<Json<Value>, ApiError>> {
        tokio::spawn(async move {
            apply_workspace_proposal_inner(
                Path((workspace_id, proposal_id)),
                State(state),
                Some(&hook),
            )
            .await
        })
    }

    async fn graph_entry_names(state: &AppState, workspace_id: &str) -> Vec<String> {
        let graph_dir = state
            .data_dir
            .join("workspaces")
            .join(workspace_id)
            .join("graphs");
        let mut entries = tokio::fs::read_dir(graph_dir).await.expect("graph dir");
        let mut names = Vec::new();
        while let Some(entry) = entries.next_entry().await.expect("graph entry") {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
        names.sort();
        names
    }

    fn assert_no_candidate_temps(entries: &[String]) {
        assert!(
            entries
                .iter()
                .all(|name| !(name.starts_with(".hf-") && name.ends_with(".tmp"))),
            "candidate temp remains: {entries:?}"
        );
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
                        size: None,
                    },
                ),
                (
                    "writer".to_owned(),
                    GraphNode {
                        node_type: "llm.prompt_writer".to_owned(),
                        title: "Prompt".to_owned(),
                        params: json!({ "style": "product" }),
                        pos: [220.0, 0.0],
                        size: None,
                    },
                ),
                (
                    "video".to_owned(),
                    GraphNode {
                        node_type: "video.text_to_video".to_owned(),
                        title: "Video".to_owned(),
                        params: json!({
                            "prompt": "clean product shot",
                            "duration_sec": 5,
                            "aspect_ratio": "9:16"
                        }),
                        pos: [440.0, 0.0],
                        size: None,
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
