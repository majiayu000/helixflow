use std::sync::Arc;

use futures_util::StreamExt;
use helixflow_run::{EventBus, RunEventEnvelope};
use helixflow_store::Store;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::time::{Duration, timeout};

use crate::app_state::AppState;
use crate::test_support::FailingWorkbenchAgent;

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

#[tokio::test]
async fn websocket_requires_workspace_id() {
    let events = EventBus::new(16);
    let (_dir, state) = test_app_state(events).await;
    let app = crate::app(state);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test listener");
    let addr = listener.local_addr().expect("listener addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve test app");
    });

    let missing = tokio_tungstenite::connect_async(format!("ws://{addr}/ws")).await;
    assert!(missing.is_err(), "missing workspace_id must be rejected");
    let empty = tokio_tungstenite::connect_async(format!("ws://{addr}/ws?workspace_id=")).await;
    assert!(empty.is_err(), "empty workspace_id must be rejected");
}

#[tokio::test]
async fn websocket_survives_broadcast_lag_and_keeps_workspace_filter() {
    let events = EventBus::new(1);
    let (_dir, state) = test_app_state(events.clone()).await;
    let app = crate::app(state);
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
            workspace_id: "ws_1".to_owned(),
            run_id: "run_1".to_owned(),
            seq: 1,
            server_time: "2026-06-12T00:00:00Z".to_owned(),
            ev: "node.state".to_owned(),
            data: json!({ "node_id": "n1", "state": "running" }),
        })
        .expect("publish first event");
    timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("first websocket message timeout")
        .expect("websocket stream item")
        .expect("websocket frame");

    for seq in 2..=20 {
        events
            .publish(RunEventEnvelope {
                workspace_id: "ws_1".to_owned(),
                run_id: "run_1".to_owned(),
                seq,
                server_time: "2026-06-12T00:00:00Z".to_owned(),
                ev: "node.state".to_owned(),
                data: json!({ "node_id": "n1", "state": "running" }),
            })
            .expect("publish burst event");
    }
    events
        .publish(RunEventEnvelope {
            workspace_id: "ws_other".to_owned(),
            run_id: "run_other".to_owned(),
            seq: 99,
            server_time: "2026-06-12T00:00:00Z".to_owned(),
            ev: "run.succeeded".to_owned(),
            data: json!({}),
        })
        .expect("publish other workspace event");
    events
        .publish(RunEventEnvelope {
            workspace_id: "ws_1".to_owned(),
            run_id: "run_1".to_owned(),
            seq: 21,
            server_time: "2026-06-12T00:00:00Z".to_owned(),
            ev: "run.succeeded".to_owned(),
            data: json!({}),
        })
        .expect("publish lag survivor");

    let mut saw_succeeded = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        let Some(message) = timeout(Duration::from_millis(200), socket.next())
            .await
            .ok()
            .flatten()
        else {
            continue;
        };
        let text = message.expect("websocket frame").into_text().expect("text");
        let body: Value = serde_json::from_str(&text).expect("event json");
        assert_ne!(body["workspace_id"], "ws_other");
        if body["ev"] == "run.succeeded" && body["run_id"] == "run_1" {
            saw_succeeded = true;
            break;
        }
    }
    assert!(
        saw_succeeded,
        "lagged websocket must keep streaming later events"
    );
}
