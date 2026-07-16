use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use helixflow_store::{NewVersion, VersionSource};

use super::{WorkspaceMessageRequest, post_workspace_message, tests};

#[tokio::test]
async fn stale_agent_proposal_returns_conflict_without_pending_record() {
    let (state, workspace_id, base_version_id, _dir) = tests::state_with_workspace().await;
    let base = state
        .store
        .version(&base_version_id)
        .await
        .expect("base version");
    let newer = state
        .store
        .create_version_after(
            NewVersion {
                workspace_id: &workspace_id,
                label: "Concurrent edit",
                source: VersionSource::Manual,
                graph_path: &base.graph_path,
                graph_hash: &base.graph_hash,
                parent_id: Some(&base_version_id),
            },
            &base_version_id,
        )
        .await
        .expect("advance current version");

    let err = post_workspace_message(
        Path(workspace_id.clone()),
        State(state.clone()),
        Json(WorkspaceMessageRequest {
            base_version_id,
            user_message: "创建一个 workflow".to_owned(),
            graph: tests::sample_graph(),
            canvas_context: None,
        }),
    )
    .await
    .expect_err("stale agent proposal must conflict");

    assert_eq!(err.status, StatusCode::CONFLICT);
    assert_eq!(
        err.message,
        "message base version is not the workspace current version"
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
            .workspace_proposals(&workspace_id)
            .await
            .expect("proposals")
            .is_empty()
    );
    assert_eq!(
        state
            .store
            .workspace(&workspace_id)
            .await
            .expect("workspace")
            .cur_version_id
            .as_deref(),
        Some(newer.id.as_str())
    );
}
