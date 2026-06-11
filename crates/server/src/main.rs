use axum::{Json, Router, routing::get};
use serde_json::{Value, json};

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/system", get(system));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8787")
        .await
        .expect("bind local server");

    axum::serve(listener, app).await.expect("serve local app");
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
