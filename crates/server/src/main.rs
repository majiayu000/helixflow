use axum::{
    Json, Router,
    routing::put,
    routing::{get, post},
};
use helixflow_run::EventBus;
use serde_json::{Value, json};

mod api_error;
mod app_state;
#[cfg(test)]
mod artifact_retry_tests;
mod artifact_routes;
mod auth;
mod canvas_collaboration;
mod canvas_ticket;
mod capability_preflight;
mod catalog_routes;
mod graph_files;
mod layout_routes;
mod ops_routes;
#[cfg(test)]
mod ops_routes_tests;
mod proposal_routes;
mod registry_routes;
mod run_routes;
#[cfg(test)]
mod run_routes_tests;
#[cfg(test)]
mod run_routes_unavailable_tests;
mod sweep_support;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod test_wait;
mod upload_routes;
mod version_file_consistency;
#[cfg(test)]
mod version_file_consistency_tests;
mod version_file_reconciliation;
#[cfg(test)]
mod version_file_reconciliation_tests;
mod version_routes;
mod workbench_message;
mod workbench_message_canvas;
mod workbench_message_graph;
#[cfg(test)]
mod workbench_message_graph_tests;
mod workbench_message_intent;
mod workbench_message_metadata;
mod workbench_message_proposals;
#[cfg(test)]
mod workbench_message_tests;
mod workbench_payload;
mod workspace_canvas;
mod workspace_events;
mod workspace_routes;
mod workspace_state;
mod workspace_state_run;
#[cfg(test)]
mod workspace_state_tests;
mod ws;

use app_state::AppState;
use artifact_routes::{
    accept_output, artifact_content, download_output, preview_output, reject_output, select_output,
};
use auth::{AuthConfig, require_auth, validate_bind_auth};
use canvas_collaboration::{apply_canvas_comment_op, update_canvas_presence};
use canvas_ticket::create_canvas_ticket;
use catalog_routes::{
    capability_models, catalog_snapshot, compile_intent, model_capabilities, resolve_implementation,
};
use layout_routes::save_workspace_layout;
use ops_routes::apply_workspace_ops;
use proposal_routes::{apply_workspace_proposal, dismiss_workspace_proposal};
use registry_routes::node_registry_catalog;
use run_routes::{confirm_run, hold_run, interrupt_active_run, queue_workspace_run};
use upload_routes::upload_workspace_image;
use version_file_reconciliation::ReconciliationReport;
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
    println!(
        "{}",
        version_file_reconciliation_event(&state.reconciliation_report)
    );
    let auth_config = AuthConfig::from_env();
    let bind_addr =
        std::env::var("HELIXFLOW_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".to_owned());
    if let Err(message) = validate_bind_auth(&bind_addr, &auth_config) {
        eprintln!("{message}");
        std::process::exit(1);
    }
    let app = serve_web_dist(app_with_auth(state, auth_config));
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("bind local server");

    axum::serve(listener, app).await.expect("serve local app");
}

/// Serve the built frontend from the same process so the release topology
/// is a single server (HF-022). Reads HELIXFLOW_WEB_DIST (default
/// `web/dist`); if the directory is missing the API still runs and `/`
/// explains how to build the frontend.
fn serve_web_dist(app: Router) -> Router {
    let web_dist = std::path::PathBuf::from(
        std::env::var("HELIXFLOW_WEB_DIST").unwrap_or_else(|_| "web/dist".to_owned()),
    );
    if !web_dist.join("index.html").exists() {
        eprintln!(
            "web dist `{}` not found; serving API only (build it with `cd web && npm run build`              or set HELIXFLOW_WEB_DIST)",
            web_dist.display()
        );
        return app;
    }
    let index = web_dist.join("index.html");
    app.fallback_service(
        tower_http::services::ServeDir::new(web_dist)
            .fallback(tower_http::services::ServeFile::new(index)),
    )
}

fn app_with_auth(state: AppState, auth_config: AuthConfig) -> Router {
    app(state)
        .layer(axum::middleware::from_fn(require_auth))
        .layer(axum::Extension(auth_config))
}

pub(crate) fn version_file_reconciliation_event(report: &ReconciliationReport) -> String {
    json!({
        "event": "version_file_reconciliation",
        "report": report,
    })
    .to_string()
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/ready", get(ready))
        .route("/api/system", get(system))
        .route("/api/registry/catalog", get(node_registry_catalog))
        .route("/api/catalog", get(catalog_snapshot))
        .route(
            "/api/catalog/capabilities/{capability_id}/models",
            get(capability_models),
        )
        .route(
            "/api/catalog/models/{model_id}/capabilities",
            get(model_capabilities),
        )
        .route("/api/catalog/resolve", post(resolve_implementation))
        .route("/api/workflows/compile-intent", post(compile_intent))
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
            "/api/workspaces/{workspace_id}/uploads",
            post(upload_workspace_image),
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

/// Readiness checks the database, storage directory, and provider state
/// instead of returning a constant `ok` (HF-029).
async fn ready(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<Json<Value>, crate::api_error::ApiError> {
    let db_ok = state.store.ping().await.is_ok();
    let probe = state
        .data_dir
        .join(format!(".readiness-probe-{}", uuid::Uuid::now_v7()));
    let storage_ok = match tokio::fs::write(&probe, b"ok").await {
        Ok(()) => {
            drop(tokio::fs::remove_file(&probe).await);
            true
        }
        Err(_) => false,
    };
    let provider_health = helixflow_gateway::Provider::health(&state.provider_registry).await;
    let body = json!({
        "ok": db_ok && storage_ok,
        "database": db_ok,
        "storage": storage_ok,
        "provider": { "ok": provider_health.ok, "message": provider_health.message },
    });
    if !(db_ok && storage_ok) {
        return Err(crate::api_error::ApiError::service_unavailable(
            body.to_string(),
        ));
    }
    Ok(Json(body))
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

    #[tokio::test]
    async fn auth_token_gates_rest_and_ws_requests() {
        use tokio::net::TcpListener;

        let events = EventBus::new(16);
        let (_dir, state) = test_app_state(events.clone()).await;
        let app = app_with_auth(state, AuthConfig::with_token(Some("secret-token")));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("listener addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let client = reqwest::Client::new();
        let denied = client
            .get(format!("http://{addr}/api/health"))
            .send()
            .await
            .expect("request without token");
        assert_eq!(denied.status(), 401);

        let wrong = client
            .get(format!("http://{addr}/api/health"))
            .bearer_auth("wrong-token")
            .send()
            .await
            .expect("request with wrong token");
        assert_eq!(wrong.status(), 401);

        let allowed = client
            .get(format!("http://{addr}/api/health"))
            .bearer_auth("secret-token")
            .send()
            .await
            .expect("request with token");
        assert_eq!(allowed.status(), 200);

        // Query-string tokens are reserved for the WebSocket route; REST
        // endpoints must use the Authorization header (GH-127).
        let query_denied = client
            .get(format!("http://{addr}/api/health?token=secret-token"))
            .send()
            .await
            .expect("request with query token");
        assert_eq!(query_denied.status(), 401);

        let ws_denied =
            tokio_tungstenite::connect_async(format!("ws://{addr}/ws?workspace_id=ws_1")).await;
        assert!(ws_denied.is_err(), "ws without token must be rejected");
        let ws_allowed = tokio_tungstenite::connect_async(format!(
            "ws://{addr}/ws?workspace_id=ws_1&token=secret-token"
        ))
        .await;
        assert!(ws_allowed.is_ok(), "ws with token must connect");
    }

    #[tokio::test]
    async fn without_configured_token_requests_pass_through() {
        use tokio::net::TcpListener;

        let events = EventBus::new(16);
        let (_dir, state) = test_app_state(events.clone()).await;
        let app = app_with_auth(state, AuthConfig::with_token(None));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("listener addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve test app");
        });

        let response = reqwest::get(format!("http://{addr}/api/health"))
            .await
            .expect("request without auth");
        assert_eq!(response.status(), 200);
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
