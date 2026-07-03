use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::{IntoResponse, Response},
};
use helixflow_run::RunEventEnvelope;
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::app_state::AppState;
use crate::canvas_ticket::validate_canvas_ws_ticket;

#[derive(Debug, Deserialize)]
pub(crate) struct WorkspaceEventQuery {
    workspace_id: Option<String>,
    ticket: Option<String>,
}

pub(crate) async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WorkspaceEventQuery>,
    State(state): State<AppState>,
) -> Response {
    if let Err(error) = validate_canvas_ws_ticket(
        &state,
        query.workspace_id.as_deref(),
        query.ticket.as_deref(),
    )
    .await
    {
        return error.into_response();
    }
    ws.on_upgrade(move |socket| stream_events(socket, state.events.subscribe(), query.workspace_id))
        .into_response()
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
