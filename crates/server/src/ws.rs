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
        tokio::select! {
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            event = receiver.recv() => {
                match event {
                    Ok(event) => {
                        if event.workspace_id != workspace_id {
                            continue;
                        }
                        if send_text(&mut socket, &event).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        let lagged = json!({
                            "workspace_id": workspace_id,
                            "run_id": workspace_id,
                            "seq": 0,
                            "server_time": "1970-01-01T00:00:00Z",
                            "ev": "ws.lagged",
                            "data": { "skipped": skipped },
                        });
                        if send_text(&mut socket, &lagged).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

async fn send_text(socket: &mut WebSocket, value: &impl serde::Serialize) -> Result<(), ()> {
    let text = serde_json::to_string(value).map_err(|_| ())?;
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}
