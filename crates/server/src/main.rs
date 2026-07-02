mod agent_transcript;
mod chat_intent;
mod design_artifact;
mod provider;
mod workbench;
mod workbench_canvas;
mod workbench_helpers;
mod workbench_view;

use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use helixflow_run::RunEventEnvelope;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use workbench::{Workbench, WorkbenchError};
use workbench_canvas::{CanvasEventsQuery, CanvasOpsRequest, CanvasPresenceRequest};

#[tokio::main]
async fn main() {
    let workbench = Workbench::open().await.expect("open workbench");
    let (canvas_events, _) = broadcast::channel(128);
    let app = app(AppState {
        workbench,
        canvas_events,
    });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8787")
        .await
        .expect("bind local server");

    axum::serve(listener, app).await.expect("serve local app");
}

#[derive(Clone)]
struct AppState {
    workbench: Workbench,
    canvas_events: broadcast::Sender<Value>,
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/system", get(system))
        .route("/api/providers", get(providers))
        .route("/api/workspaces", get(workspaces))
        .route("/api/workspaces/{workspace_id}/state", get(workspace_state))
        .route(
            "/api/workspaces/{workspace_id}/canvas",
            get(workspace_canvas),
        )
        .route("/api/canvases/{canvas_id}/ops", post(post_canvas_ops))
        .route("/api/canvases/{canvas_id}/events", get(canvas_events))
        .route(
            "/api/canvases/{canvas_id}/presence",
            post(post_canvas_presence),
        )
        .route("/api/canvases/{canvas_id}/ws", get(canvas_ws_handler))
        .route(
            "/api/workspaces/{workspace_id}/messages",
            post(post_message),
        )
        .route("/api/workspaces/{workspace_id}/runs", post(request_run))
        .route(
            "/api/workspaces/{workspace_id}/proposals/{proposal_id}/apply",
            post(apply_proposal),
        )
        .route(
            "/api/workspaces/{workspace_id}/proposals/{proposal_id}/dismiss",
            post(dismiss_proposal),
        )
        .route(
            "/api/workspaces/{workspace_id}/confirmations/{run_id}/hold",
            post(hold_run),
        )
        .route(
            "/api/workspaces/{workspace_id}/confirmations/{run_id}/approve",
            post(approve_run),
        )
        .route("/ws", get(ws_handler))
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "service": "helixflow" }))
}

async fn system() -> Json<Value> {
    Json(json!({
        "agent": helixflow_agent::module_name(),
        "gateway": helixflow_gateway::module_name(),
        "graph": helixflow_graph::module_name(),
        "registry": helixflow_registry::module_name(),
        "run": helixflow_run::module_name(),
        "store": helixflow_store::module_name()
    }))
}

async fn providers(State(state): State<AppState>) -> Json<Value> {
    Json(state.workbench.provider_status().await)
}

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

async fn workspaces(State(state): State<AppState>) -> ApiResult {
    let workspaces = state
        .workbench
        .store
        .workspaces()
        .await
        .map_err(WorkbenchError::from)
        .map_err(api_error)?;
    Ok(Json(json!({
        "workspaces": workspaces.iter().map(|workspace| json!({
            "id": workspace.id,
            "name": workspace.name,
            "versionId": workspace.cur_version_id,
            "createdAt": workspace.created_at,
            "updatedAt": workspace.updated_at,
            "firstMessage": workspace.first_message,
            "messageCount": workspace.message_count
        })).collect::<Vec<_>>()
    })))
}

async fn workspace_state(
    State(state): State<AppState>,
    Path(workspace_id): Path<String>,
) -> ApiResult {
    state
        .workbench
        .state(&workspace_id)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn workspace_canvas(
    State(state): State<AppState>,
    Path(workspace_id): Path<String>,
) -> ApiResult {
    state
        .workbench
        .canvas_for_workspace(&workspace_id)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn post_canvas_ops(
    State(state): State<AppState>,
    Path(canvas_id): Path<String>,
    Json(request): Json<CanvasOpsRequest>,
) -> ApiResult {
    let response = state
        .workbench
        .append_canvas_ops(&canvas_id, request)
        .await
        .map_err(api_error)?;
    if let Some(ops) = response.get("ops").and_then(Value::as_array) {
        for op in ops {
            publish_canvas_event(
                &state.canvas_events,
                json!({
                    "type": "op",
                    "canvas_id": canvas_id,
                    "op": op
                }),
            );
        }
    }
    Ok(Json(response))
}

async fn canvas_events(
    State(state): State<AppState>,
    Path(canvas_id): Path<String>,
    Query(query): Query<CanvasEventsQuery>,
) -> ApiResult {
    state
        .workbench
        .canvas_events_after(&canvas_id, query.after_seq.unwrap_or(0))
        .await
        .map(Json)
        .map_err(api_error)
}

async fn post_canvas_presence(
    State(state): State<AppState>,
    Path(canvas_id): Path<String>,
    Json(request): Json<CanvasPresenceRequest>,
) -> ApiResult {
    let response = state
        .workbench
        .upsert_canvas_presence(&canvas_id, request)
        .await
        .map_err(api_error)?;
    publish_canvas_event(
        &state.canvas_events,
        json!({
            "type": "presence",
            "canvas_id": canvas_id,
            "presence": response.get("presence").cloned().unwrap_or(Value::Null)
        }),
    );
    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    text: String,
}

async fn post_message(
    State(state): State<AppState>,
    Path(workspace_id): Path<String>,
    Json(request): Json<ChatRequest>,
) -> ApiResult {
    state
        .workbench
        .send_message(&workspace_id, &request.text)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn request_run(State(state): State<AppState>, Path(workspace_id): Path<String>) -> ApiResult {
    state
        .workbench
        .request_run(&workspace_id)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn apply_proposal(
    State(state): State<AppState>,
    Path((workspace_id, proposal_id)): Path<(String, String)>,
) -> ApiResult {
    state
        .workbench
        .apply_proposal(&workspace_id, &proposal_id)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn dismiss_proposal(
    State(state): State<AppState>,
    Path((workspace_id, proposal_id)): Path<(String, String)>,
) -> ApiResult {
    state
        .workbench
        .dismiss_proposal(&workspace_id, &proposal_id)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn approve_run(
    State(state): State<AppState>,
    Path((workspace_id, run_id)): Path<(String, String)>,
) -> ApiResult {
    state
        .workbench
        .approve_run(&workspace_id, &run_id)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn hold_run(
    State(state): State<AppState>,
    Path((workspace_id, run_id)): Path<(String, String)>,
) -> ApiResult {
    state
        .workbench
        .hold_run(&workspace_id, &run_id)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn canvas_ws_handler(
    ws: WebSocketUpgrade,
    Path(canvas_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        stream_canvas_events(socket, state.canvas_events.subscribe(), canvas_id)
    })
}

async fn stream_canvas_events(
    mut socket: WebSocket,
    mut receiver: broadcast::Receiver<Value>,
    canvas_id: String,
) {
    while let Ok(event) = receiver.recv().await {
        if event
            .get("canvas_id")
            .and_then(Value::as_str)
            .is_some_and(|id| id != canvas_id)
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

fn publish_canvas_event(sender: &broadcast::Sender<Value>, event: Value) {
    if let Err(broadcast::error::SendError(_event)) = sender.send(event) {
        // No active canvas websocket subscribers is a normal local state.
    }
}

fn api_error(err: WorkbenchError) -> (StatusCode, Json<Value>) {
    let status = err.status_code();
    (status, Json(json!({ "error": err.to_string() })))
}

#[derive(Debug, Deserialize)]
struct WorkspaceEventQuery {
    workspace_id: Option<String>,
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WorkspaceEventQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        stream_events(
            socket,
            state.workbench.events().subscribe(),
            query.workspace_id,
        )
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_reports_service_status() {
        let body = health().await.0;

        assert_eq!(body["ok"], true);
        assert_eq!(body["service"], "helixflow");
    }

    #[tokio::test]
    async fn system_reports_workspace_modules() {
        let body = system().await.0;

        assert_eq!(body["agent"], "agent");
        assert_eq!(body["gateway"], "gateway");
        assert_eq!(body["graph"], "graph");
        assert_eq!(body["registry"], "registry");
        assert_eq!(body["run"], "run");
        assert_eq!(body["store"], "store");
    }

    #[test]
    fn app_exposes_websocket_event_payload_shape() {
        let event = RunEventEnvelope {
            workspace_id: "ws_1".to_owned(),
            run_id: "run_1".to_owned(),
            seq: 7,
            server_time: "2026-06-12T00:00:00Z".to_owned(),
            ev: "node.state".to_owned(),
            data: json!({ "node_id": "n1", "state": "running" }),
        };

        let encoded = serde_json::to_value(event).expect("serialize event");

        assert_eq!(encoded["run_id"], "run_1");
        assert_eq!(encoded["workspace_id"], "ws_1");
        assert_eq!(encoded["ev"], "node.state");
        assert_eq!(encoded["data"]["state"], "running");
    }
}
