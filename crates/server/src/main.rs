use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use helixflow_run::{EventBus, RunEventEnvelope};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::broadcast;

#[tokio::main]
async fn main() {
    let app = app(AppState::new(EventBus::default()));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8787")
        .await
        .expect("bind local server");

    axum::serve(listener, app).await.expect("serve local app");
}

#[derive(Clone, Default)]
struct AppState {
    events: EventBus,
}

impl AppState {
    fn new(events: EventBus) -> Self {
        Self { events }
    }
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/system", get(system))
        .route("/api/workspaces/{workspace_id}/state", get(workspace_state))
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

async fn workspace_state(Path(workspace_id): Path<String>) -> Json<Value> {
    Json(workspace_state_payload(&workspace_id))
}

fn workspace_state_payload(workspace_id: &str) -> Value {
    json!({
        "eventSeq": 0,
        "workspace": {
            "id": workspace_id,
            "name": "Helixflow Demo",
            "versionId": "ver_demo_1",
            "updatedAt": "2026-06-12T00:00:00Z"
        },
        "chat": {
            "messages": [
                {
                    "id": "msg_1",
                    "role": "user",
                    "text": "Create a vertical product teaser from the launch note.",
                    "time": "09:10"
                },
                {
                    "id": "msg_2",
                    "role": "agent",
                    "text": "Graph proposal is ready for review.",
                    "time": "09:11"
                }
            ]
        },
        "graph": {
            "nodes": [
                {
                    "id": "text",
                    "nodeType": "input.text",
                    "title": "Launch note",
                    "category": "Input",
                    "status": "succeeded",
                    "position": { "x": 48, "y": 158 },
                    "provider": null,
                    "summary": "Source copy"
                },
                {
                    "id": "writer",
                    "nodeType": "llm.prompt_writer",
                    "title": "Prompt writer",
                    "category": "Text",
                    "status": "succeeded",
                    "position": { "x": 235, "y": 96 },
                    "provider": "mock",
                    "summary": "Cinematic prompt"
                },
                {
                    "id": "video",
                    "nodeType": "video.mock.text_to_video",
                    "title": "Video render",
                    "category": "Video",
                    "status": "queued",
                    "position": { "x": 405, "y": 156 },
                    "provider": "mock",
                    "summary": "9:16, 4 seconds"
                },
                {
                    "id": "save",
                    "nodeType": "output.save",
                    "title": "Save output",
                    "category": "Output",
                    "status": "queued",
                    "position": { "x": 565, "y": 226 },
                    "provider": null,
                    "summary": "Selected artifact"
                }
            ],
            "edges": [
                {
                    "id": "edge_text_writer",
                    "from": { "nodeId": "text", "port": "text" },
                    "to": { "nodeId": "writer", "port": "text" },
                    "kind": "text"
                },
                {
                    "id": "edge_writer_video",
                    "from": { "nodeId": "writer", "port": "prompt" },
                    "to": { "nodeId": "video", "port": "prompt" },
                    "kind": "text"
                },
                {
                    "id": "edge_video_save",
                    "from": { "nodeId": "video", "port": "video" },
                    "to": { "nodeId": "save", "port": "artifact" },
                    "kind": "artifact"
                }
            ]
        },
        "run": {
            "id": "run_demo_1",
            "label": "Manual preview",
            "status": "running",
            "steps": [
                { "nodeId": "text", "title": "Launch note", "state": "succeeded", "provider": null },
                { "nodeId": "writer", "title": "Prompt writer", "state": "succeeded", "provider": "mock" },
                { "nodeId": "video", "title": "Video render", "state": "queued", "provider": "mock" },
                { "nodeId": "save", "title": "Save output", "state": "queued", "provider": null }
            ],
            "cost": { "estimate": 0, "actual": 0, "currency": "USD" }
        },
        "outputs": [
            {
                "id": "art_prompt_1",
                "kind": "text",
                "title": "Prompt draft",
                "storageUri": "workspace://outputs/run_demo_1/writer/prompt_writer.txt",
                "selected": false,
                "meta": "Generated prompt"
            },
            {
                "id": "art_video_1",
                "kind": "video",
                "title": "Vertical teaser",
                "storageUri": "workspace://outputs/run_demo_1/video/text_to_video.mp4",
                "selected": true,
                "meta": "1080 x 1920"
            }
        ],
        "history": [
            {
                "id": "hist_1",
                "kind": "version",
                "label": "Initial graph",
                "time": "09:05",
                "summary": "Version ver_demo_1"
            },
            {
                "id": "hist_2",
                "kind": "run",
                "label": "Manual preview",
                "time": "09:12",
                "summary": "Run run_demo_1"
            }
        ],
        "pendingConfirmation": {
            "id": "confirm_1",
            "title": "Agent requested run",
            "summary": "Run the current graph through the mock provider.",
            "cost": { "amount": 0, "currency": "USD" }
        }
    })
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

    #[tokio::test]
    async fn workspace_state_hydrates_workbench_shell() {
        let body = workspace_state(Path("demo".to_owned())).await.0;

        assert_eq!(body["workspace"]["id"], "demo");
        assert_eq!(body["graph"]["nodes"].as_array().expect("nodes").len(), 4);
        assert_eq!(body["run"]["steps"].as_array().expect("steps").len(), 4);
        assert!(body["pendingConfirmation"].is_object());
    }

    #[test]
    fn app_exposes_websocket_event_payload_shape() {
        let _app = app(AppState::default());
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

    #[tokio::test]
    async fn websocket_streams_injected_event_bus() {
        use futures_util::StreamExt;
        use tokio::net::TcpListener;
        use tokio::time::{Duration, timeout};

        let events = EventBus::new(16);
        let app = app(AppState::new(events.clone()));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("listener addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let (mut socket, _) =
            tokio_tungstenite::connect_async(format!("ws://{addr}/ws?workspace_id=ws_1"))
                .await
                .expect("connect websocket");
        events
            .publish(RunEventEnvelope {
                workspace_id: "ws_other".to_owned(),
                run_id: "run_other".to_owned(),
                seq: 1,
                server_time: "2026-06-12T00:00:00Z".to_owned(),
                ev: "node.state".to_owned(),
                data: json!({ "node_id": "n1", "state": "failed" }),
            })
            .expect("publish other event");
        events
            .publish(RunEventEnvelope {
                workspace_id: "ws_1".to_owned(),
                run_id: "run_1".to_owned(),
                seq: 1,
                server_time: "2026-06-12T00:00:00Z".to_owned(),
                ev: "node.state".to_owned(),
                data: json!({ "node_id": "n1", "state": "running" }),
            })
            .expect("publish event");

        let message = timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("websocket message")
            .expect("websocket stream item")
            .expect("websocket frame");
        let text = message.into_text().expect("text frame");
        let body: Value = serde_json::from_str(&text).expect("event json");

        assert_eq!(body["workspace_id"], "ws_1");
        assert_eq!(body["run_id"], "run_1");
        assert_eq!(body["ev"], "node.state");
        assert_eq!(body["data"]["state"], "running");
    }
}
