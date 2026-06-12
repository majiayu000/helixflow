use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use helixflow_run::{EventBus, RunEventEnvelope};
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

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| stream_events(socket, state.events.subscribe()))
}

async fn stream_events(mut socket: WebSocket, mut receiver: broadcast::Receiver<RunEventEnvelope>) {
    while let Ok(event) = receiver.recv().await {
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

        let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
            .await
            .expect("connect websocket");
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
