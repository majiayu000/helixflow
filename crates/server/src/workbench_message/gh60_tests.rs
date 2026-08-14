use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use helixflow_agent::TurnMode;
use serde_json::Value;

use super::tests::{sample_graph, state_with_workspace};
use super::{WorkspaceMessageRequest, post_workspace_message};

#[tokio::test]
async fn post_message_routes_ambiguous_text_to_chat() {
    let (state, workspace_id, version_id, _dir) = state_with_workspace().await;

    let response = post_workspace_message(
        Path(workspace_id.clone()),
        State(state.clone()),
        Json(WorkspaceMessageRequest {
            base_version_id: version_id,
            user_message: "继续".to_owned(),
            graph: sample_graph(),
            canvas_context: None,
            conversation_id: None,
            turn_mode: None,
        }),
    )
    .await
    .expect("ambiguous text should route to chat")
    .0;

    assert_eq!(response.turn_mode, TurnMode::Chat);
    assert_eq!(response.proposal, None);
    assert_eq!(response.run, None);
    assert_eq!(response.pending_confirmation, None);
    assert_eq!(response.messages[0].kind, "chat");

    let persisted = state
        .store
        .workspace_messages(&workspace_id)
        .await
        .expect("messages");
    assert_eq!(persisted[0].text.as_deref(), Some("继续"));
    let metadata: Value = serde_json::from_str(
        persisted[0]
            .attachment_ids_json
            .as_deref()
            .expect("metadata"),
    )
    .expect("metadata json");
    assert_eq!(metadata["turnMode"], "chat");
    assert_eq!(metadata["turnModeSource"], "ambiguous_fallback");
}

#[tokio::test]
async fn post_message_rejects_empty_text() {
    let (state, workspace_id, version_id, _dir) = state_with_workspace().await;

    let err = post_workspace_message(
        Path(workspace_id),
        State(state),
        Json(WorkspaceMessageRequest {
            base_version_id: version_id,
            user_message: "   ".to_owned(),
            graph: sample_graph(),
            canvas_context: None,
            conversation_id: None,
            turn_mode: None,
        }),
    )
    .await
    .expect_err("empty text should stay invalid");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
    assert!(err.message.contains("empty agent turn"));
}
