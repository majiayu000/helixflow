use axum::{
    Json, Router,
    routing::put,
    routing::{get, post},
};
use helixflow_run::EventBus;
use serde_json::{Value, json};

mod api_error;
mod app_state;
mod artifact_routes;
mod canvas_collaboration;
mod canvas_ticket;
mod graph_files;
mod layout_routes;
mod ops_routes;
#[cfg(test)]
mod ops_routes_tests;
mod proposal_routes;
mod registry_routes;
mod run_routes;
#[cfg(test)]
mod run_routes_unavailable_tests;
mod sweep_support;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod test_wait;
mod version_routes;
mod workbench_message;
mod workbench_message_canvas;
mod workbench_message_metadata;
mod workbench_message_proposals;
mod workbench_payload;
mod workspace_canvas;
mod workspace_events;
mod workspace_routes;
mod workspace_state;
mod workspace_state_run;
mod ws;

use app_state::AppState;
use artifact_routes::{
    accept_output, artifact_content, download_output, preview_output, reject_output, select_output,
};
use canvas_collaboration::{apply_canvas_comment_op, update_canvas_presence};
use canvas_ticket::create_canvas_ticket;
use layout_routes::save_workspace_layout;
use ops_routes::apply_workspace_ops;
use proposal_routes::{apply_workspace_proposal, dismiss_workspace_proposal};
use registry_routes::node_registry_catalog;
use run_routes::{confirm_run, hold_run, interrupt_active_run, queue_workspace_run};
use version_routes::{export_workflow_version, restore_workspace_version, undo_workspace_version};
use workbench_message::post_workspace_message;
use workspace_canvas::workspace_canvas;
use workspace_events::workspace_events;
use workspace_routes::{create_workspace, list_workspaces, set_workspace_provider};
use workspace_state::workspace_state;
use ws::ws_handler;

#[tokio::main]
async fn main() {
    let state = AppState::open(EventBus::default())
        .await
        .expect("initialize app state");
    let app = app(state);

    let bind_addr =
        std::env::var("HELIXFLOW_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".to_owned());
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("bind local server");

    axum::serve(listener, app).await.expect("serve local app");
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/system", get(system))
        .route("/api/registry/catalog", get(node_registry_catalog))
        .route(
            "/api/workspaces",
            get(list_workspaces).post(create_workspace),
        )
        .route("/api/workspaces/{workspace_id}/state", get(workspace_state))
        .route(
            "/api/workspaces/{workspace_id}/canvas",
            get(workspace_canvas),
        )
        .route(
            "/api/workspaces/{workspace_id}/canvas/comments/ops",
            post(apply_canvas_comment_op),
        )
        .route(
            "/api/workspaces/{workspace_id}/canvas/presence",
            post(update_canvas_presence),
        )
        .route(
            "/api/workspaces/{workspace_id}/events",
            get(workspace_events),
        )
        .route(
            "/api/canvases/{canvas_id}/ticket",
            post(create_canvas_ticket),
        )
        .route(
            "/api/workspaces/{workspace_id}/provider",
            put(set_workspace_provider),
        )
        .route(
            "/api/workspaces/{workspace_id}/messages",
            post(post_workspace_message),
        )
        .route(
            "/api/workspaces/{workspace_id}/proposals/{proposal_id}/apply",
            post(apply_workspace_proposal),
        )
        .route(
            "/api/workspaces/{workspace_id}/proposals/{proposal_id}/dismiss",
            post(dismiss_workspace_proposal),
        )
        .route(
            "/api/workspaces/{workspace_id}/versions/ops",
            post(apply_workspace_ops),
        )
        .route(
            "/api/workspaces/{workspace_id}/runs/{run_id}/confirm",
            post(confirm_run),
        )
        .route(
            "/api/workspaces/{workspace_id}/runs/{run_id}/hold",
            post(hold_run),
        )
        .route(
            "/api/workspaces/{workspace_id}/runs",
            post(queue_workspace_run),
        )
        .route("/api/runs/{run_id}/interrupt", post(interrupt_active_run))
        .route("/api/outputs/{output_id}/select", post(select_output))
        .route("/api/outputs/{output_id}/accept", post(accept_output))
        .route("/api/outputs/{output_id}/reject", post(reject_output))
        .route("/api/outputs/{output_id}/preview", get(preview_output))
        .route("/api/outputs/{output_id}/download", get(download_output))
        .route("/api/artifacts/{output_id}/content", get(artifact_content))
        .route(
            "/api/versions/{version_id}/export",
            get(export_workflow_version),
        )
        .route(
            "/api/workspaces/{workspace_id}/versions/undo",
            post(undo_workspace_version),
        )
        .route(
            "/api/workspaces/{workspace_id}/versions/layout",
            post(save_workspace_layout),
        )
        .route(
            "/api/workspaces/{workspace_id}/versions/{version_id}/restore",
            post(restore_workspace_version),
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use helixflow_run::RunEventEnvelope;
    use helixflow_store::Store;

    use super::*;
    use crate::test_support::FailingWorkbenchAgent;

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

    #[tokio::test]
    async fn websocket_streams_injected_event_bus() {
        use futures_util::StreamExt;
        use tokio::net::TcpListener;
        use tokio::time::{Duration, timeout};

        let events = EventBus::new(16);
        let (_dir, state) = test_app_state(events.clone()).await;
        let app = app(state);
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

    async fn test_app_state(events: EventBus) -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let state = AppState::with_store_agent(
            events,
            store,
            data_dir.clone(),
            Arc::new(FailingWorkbenchAgent),
            data_dir.join("sessions"),
        );
        (dir, state)
    }
}
