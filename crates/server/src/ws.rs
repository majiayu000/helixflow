use axum::{
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::{IntoResponse, Response},
};
use helixflow_run::RunEventEnvelope;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::broadcast;

use crate::api_error::ApiError;
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
    let Some(workspace_id) = query
        .workspace_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
    else {
        return ApiError::bad_request("workspace_id is required").into_response();
    };
    if let Err(error) =
        validate_canvas_ws_ticket(&state, Some(&workspace_id), query.ticket.as_deref()).await
    {
        return error.into_response();
    }
    ws.on_upgrade(move |socket| stream_events(socket, state.events.subscribe(), workspace_id))
        .into_response()
}

async fn stream_events(
    mut socket: WebSocket,
    mut receiver: broadcast::Receiver<RunEventEnvelope>,
    workspace_id: String,
) {
    loop {
        let event = match receiver.recv().await {
            Ok(event) => event,
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                let Ok(text) = serde_json::to_string(&json!({
                    "workspace_id": workspace_id,
                    "run_id": workspace_id,
                    "seq": 0,
                    "server_time": "1970-01-01T00:00:00Z",
                    "ev": "ws.lagged",
                    "data": { "skipped": skipped },
                })) else {
                    break;
                };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => break,
        };
        if event.workspace_id != workspace_id {
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
