use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use helixflow_run::RunEventEnvelope;
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::app_state::AppState;

#[derive(Debug, Deserialize)]
pub(crate) struct WorkspaceEventQuery {
    workspace_id: Option<String>,
}

pub(crate) async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WorkspaceEventQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| stream_events(socket, state.events.subscribe(), query.workspace_id))
}

async fn stream_events(
    mut socket: WebSocket,
    mut receiver: broadcast::Receiver<RunEventEnvelope>,
    workspace_id: Option<String>,
) {
    while let Ok(event) = receiver.recv().await {
        if workspace_id
            .as_deref()
            .is_some_and(|id| id != event.workspace_id)
        {
            continue;
        }
        let Ok(text) = serde_json::to_string(&event) else {
            break;
        };
        if socket.send(Message::Text(text.into())).await.is_err() {
            break;
        }
    }
}
