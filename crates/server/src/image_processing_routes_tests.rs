use std::sync::Arc;

use helixflow_gateway::{ApiProviderConfig, AtlasProvider, RuntimeProvider};
use helixflow_run::EventBus;
use helixflow_store::Store;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{app, app_state::AppState, test_support::FailingWorkbenchAgent};

#[tokio::test]
async fn image_processing_is_owned_by_helixflow_end_to_end() {
    let atlas_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake Atlas");
    let atlas_address = atlas_listener.local_addr().expect("fake Atlas address");
    let png = one_pixel_png();
    let atlas = tokio::spawn(async move {
        let (upload, stream) = read_request(&atlas_listener).await;
        assert!(upload.starts_with("POST /api/v1/model/uploadMedia HTTP/1.1"));
        respond_json(
            stream,
            &format!(r#"{{"url":"http://{atlas_address}/input.png"}}"#),
        )
        .await;

        let (generate, stream) = read_request(&atlas_listener).await;
        assert!(generate.starts_with("POST /api/v1/model/generateImage HTTP/1.1"));
        assert!(generate.contains("atlascloud/photo-cleanup"));
        respond_json(
            stream,
            &format!(
                r#"{{"data":{{"id":"provider-task-1","outputs":["http://{atlas_address}/output.png"]}}}}"#
            ),
        )
        .await;

        let (download, stream) = read_request(&atlas_listener).await;
        assert!(download.starts_with("GET /output.png HTTP/1.1"));
        respond_bytes(stream, "image/png", &png).await;
    });

    let directory = tempfile::tempdir().expect("temp dir");
    let data_dir = directory.path().to_path_buf();
    let store = Store::open(&format!(
        "sqlite://{}",
        data_dir.join("helixflow.sqlite").display()
    ))
    .await
    .expect("open store");
    let workspace = store
        .create_workspace("Image test")
        .await
        .expect("workspace");
    let provider = RuntimeProvider::Atlas(AtlasProvider::new(ApiProviderConfig::atlas(
        "test-key".to_owned(),
        format!("http://{atlas_address}/v1"),
    )));
    let state = AppState::with_store_agent_provider(
        EventBus::new(16),
        store.clone(),
        data_dir.clone(),
        Arc::new(FailingWorkbenchAgent),
        data_dir.join("sessions"),
        provider,
    );
    let helix_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind Helixflow");
    let helix_address = helix_listener.local_addr().expect("Helixflow address");
    let server = tokio::spawn(async move {
        axum::serve(helix_listener, app(state))
            .await
            .expect("serve");
    });

    let response = reqwest::Client::new()
        .post(format!(
            "http://{helix_address}/api/workspaces/{}/image-processing-jobs",
            workspace.id
        ))
        .multipart(
            reqwest::multipart::Form::new()
                .text(
                    "request",
                    json!({
                        "sourceNodeId": "photo",
                        "intent": "enhance",
                        "parameters": {}
                    })
                    .to_string(),
                )
                .part(
                    "file",
                    reqwest::multipart::Part::bytes(one_pixel_png())
                        .file_name("photo.png")
                        .mime_str("image/png")
                        .expect("mime"),
                ),
        )
        .send()
        .await
        .expect("execute image processing");
    assert_eq!(response.status(), reqwest::StatusCode::ACCEPTED);
    let payload: serde_json::Value = response.json().await.expect("execution response");
    assert_eq!(payload["job"]["status"], "queued");
    let job_id = payload["job"]["id"].as_str().expect("job id");
    let client = reqwest::Client::new();
    let mut completed_job = None;
    for _ in 0..100 {
        let response = client
            .get(format!(
                "http://{helix_address}/api/workspaces/{}/image-processing-jobs/{job_id}",
                workspace.id
            ))
            .send()
            .await
            .expect("read image processing job");
        let job: serde_json::Value = response.json().await.expect("job response");
        if matches!(job["status"].as_str(), Some("succeeded" | "failed")) {
            completed_job = Some(job);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let completed_job = completed_job.expect("image job reaches a terminal state");
    assert_eq!(completed_job["status"], "succeeded");
    assert_eq!(completed_job["provider"], "atlas");
    assert_eq!(completed_job["model"], "atlascloud/photo-cleanup");
    assert_eq!(completed_job["providerTaskId"], "provider-task-1");
    let upload_id = completed_job["outputUploadId"]
        .as_str()
        .expect("output upload id");
    let completed = reqwest::Client::new()
        .put(format!(
            "http://{helix_address}/api/workspaces/{}/image-processing-jobs/{job_id}",
            workspace.id
        ))
        .json(&json!({
            "resultNodeId": "image_enhance",
            "outputUploadId": upload_id
        }))
        .send()
        .await
        .expect("complete image processing");
    assert_eq!(completed.status(), reqwest::StatusCode::OK);
    let record = store
        .workspace_image_processing_job(&workspace.id, job_id)
        .await
        .expect("durable job");
    assert_eq!(record.status, "succeeded");
    assert_eq!(record.result_node_id.as_deref(), Some("image_enhance"));
    assert_eq!(record.provider_task_id.as_deref(), Some("provider-task-1"));

    atlas.await.expect("fake Atlas task");
    server.abort();
}

fn one_pixel_png() -> Vec<u8> {
    hex::decode("89504e470d0a1a0a0000000d4948445200000001000000010802000000907753de00000009704859730000000100000001004f25c4d60000000c49444154789c636460600000000800023be8c1870000000049454e44ae426082")
        .expect("valid PNG fixture")
}

async fn read_request(listener: &tokio::net::TcpListener) -> (String, tokio::net::TcpStream) {
    let (mut stream, _) = listener.accept().await.expect("accept request");
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let count = stream.read(&mut chunk).await.expect("read request");
        assert!(count > 0);
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let header = String::from_utf8_lossy(&bytes[..header_end]);
    let content_length = header
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length: ")
                .and_then(|value| value.parse::<usize>().ok())
        })
        .unwrap_or(0);
    while bytes.len() < header_end + content_length {
        let mut chunk = [0_u8; 4096];
        let count = stream.read(&mut chunk).await.expect("read body");
        assert!(count > 0);
        bytes.extend_from_slice(&chunk[..count]);
    }
    (String::from_utf8_lossy(&bytes).into_owned(), stream)
}

async fn respond_json(stream: tokio::net::TcpStream, body: &str) {
    respond_bytes(stream, "application/json", body.as_bytes()).await;
}

async fn respond_bytes(mut stream: tokio::net::TcpStream, mime: &str, body: &[u8]) {
    stream
        .write_all(
            format!(
                "HTTP/1.1 200 OK\r\ncontent-type: {mime}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .expect("write response headers");
    stream.write_all(body).await.expect("write response body");
}
