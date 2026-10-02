use std::{io::Cursor, sync::Arc, time::Duration};

use helixflow_gateway::{ApiProviderConfig, AtlasProvider, RuntimeProvider};
use helixflow_run::EventBus;
use helixflow_store::{ImageProcessingJobUpdate, NewImageProcessingJob, NewUpload, Store};
use image::{DynamicImage, ImageFormat, RgbaImage};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{app, app_state::AppState, test_support::FailingWorkbenchAgent};

#[tokio::test]
async fn image_processing_is_owned_by_helixflow_end_to_end() {
    assert_image_processing_output(one_pixel_png(), None).await;
}

#[tokio::test]
async fn image_processing_output_above_upload_limit_succeeds() {
    assert!(std::env::var_os("HELIXFLOW_MAX_UPLOAD_BYTES").is_none());
    let png = encode_png(noisy_image(2304));
    assert!(png.len() > 16 * 1024 * 1024);
    assert!(png.len() <= 32 * 1024 * 1024);
    let stored_bytes = assert_image_processing_output(png, None).await;
    assert!(stored_bytes > 16 * 1024 * 1024);
}

#[tokio::test]
async fn image_processing_reencoded_output_above_processing_limit_fails() {
    let image = DynamicImage::ImageRgba8(noisy_image(4096)).into_rgb8();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 75)
        .encode_image(&DynamicImage::ImageRgb8(image))
        .expect("encode provider JPEG");
    assert!(jpeg.len() <= 32 * 1024 * 1024);
    assert_image_processing_output(jpeg, Some("uploaded file exceeds the 33554432 byte limit"))
        .await;
}

async fn assert_image_processing_output(output: Vec<u8>, expected_error: Option<&str>) -> usize {
    let image = image::load_from_memory(&output).expect("provider output fixture");
    let source = encode_png(RgbaImage::from_pixel(
        image.width(),
        image.height(),
        image::Rgba([0, 0, 0, 255]),
    ));
    let atlas_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake Atlas");
    let atlas_address = atlas_listener.local_addr().expect("fake Atlas address");
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
        assert!(!download.to_ascii_lowercase().contains("authorization"));
        assert!(!download.contains("test-key"));
        respond_bytes(stream, "application/octet-stream", &output).await;
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
    let provider = RuntimeProvider::Atlas(
        AtlasProvider::new(ApiProviderConfig::atlas(
            "test-key".to_owned(),
            format!("http://{atlas_address}/v1"),
        ))
        .with_loopback_output_http(),
    );
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
                    reqwest::multipart::Part::bytes(source)
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
    let completed_job = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
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
                break job;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("image job reaches a terminal state");
    if let Some(expected_error) = expected_error {
        assert_eq!(completed_job["status"], "failed");
        assert_eq!(completed_job["error"], expected_error);
        assert!(completed_job["outputUploadId"].is_null());
        assert!(!data_dir.join("uploads").exists());
        atlas.await.expect("fake Atlas task");
        server.abort();
        return 0;
    }
    assert_eq!(completed_job["status"], "succeeded", "{completed_job}");
    assert_eq!(completed_job["provider"], "atlas");
    assert_eq!(completed_job["model"], "atlascloud/photo-cleanup");
    assert_eq!(completed_job["providerTaskId"], "provider-task-1");
    let upload_id = completed_job["outputUploadId"]
        .as_str()
        .expect("output upload id");
    let upload = store.upload(upload_id).await.expect("persisted output");
    let stored = tokio::fs::read(data_dir.join(&upload.file_path))
        .await
        .expect("persisted PNG bytes");
    assert!(stored.len() <= 32 * 1024 * 1024);
    let stored_bytes = stored.len();
    assert_eq!(upload.mime.as_deref(), Some("image/png"));
    image::load_from_memory(&stored).expect("valid stored PNG");
    if stored.len() > 16 * 1024 * 1024 {
        let response = client
            .post(format!(
                "http://{helix_address}/api/workspaces/{}/uploads",
                workspace.id
            ))
            .multipart(reqwest::multipart::Form::new().part(
                "file",
                reqwest::multipart::Part::bytes(stored).file_name("large.png"),
            ))
            .send()
            .await
            .expect("user upload request");
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    }
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
    stored_bytes
}

fn noisy_image(edge: u32) -> RgbaImage {
    let mut seed = 1_u32;
    RgbaImage::from_fn(edge, edge, |_, _| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        image::Rgba(seed.to_le_bytes())
    })
}

fn encode_png(image: RgbaImage) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut output, ImageFormat::Png)
        .expect("encode PNG fixture");
    output.into_inner()
}

#[tokio::test]
async fn open_for_test_interrupts_queued_and_running_image_jobs() {
    let directory = tempfile::tempdir().expect("temp dir");
    let data_dir = directory.path().to_path_buf();
    let store = Store::open(&format!(
        "sqlite://{}",
        data_dir.join("helixflow.sqlite").display()
    ))
    .await
    .expect("open store");
    let workspace = store
        .create_workspace("Images")
        .await
        .expect("create workspace");
    let upload = store
        .create_upload(NewUpload {
            workspace_id: &workspace.id,
            filename: "result.png",
            file_path: "uploads/result.png",
            sha256: "sha256:result",
            mime: Some("image/png"),
        })
        .await
        .expect("create upload");
    let queued = store
        .create_image_processing_job(NewImageProcessingJob {
            workspace_id: &workspace.id,
            source_node_id: "photo",
            intent: "enhance",
            profile: None,
        })
        .await
        .expect("queue job");
    let running = store
        .create_image_processing_job(NewImageProcessingJob {
            workspace_id: &workspace.id,
            source_node_id: "photo",
            intent: "upscale",
            profile: None,
        })
        .await
        .expect("create running job");
    store
        .update_image_processing_job(
            &workspace.id,
            &running.id,
            ImageProcessingJobUpdate {
                status: "running",
                provider_task_id: Some("provider-task-running"),
                provider: Some("atlas"),
                model: Some("atlascloud/photo-cleanup"),
                result_node_id: None,
                output_upload_id: Some(&upload.id),
                error: None,
            },
        )
        .await
        .expect("mark running");
    let succeeded = store
        .create_image_processing_job(NewImageProcessingJob {
            workspace_id: &workspace.id,
            source_node_id: "photo",
            intent: "cutout",
            profile: None,
        })
        .await
        .expect("create succeeded job");
    store
        .update_image_processing_job(
            &workspace.id,
            &succeeded.id,
            ImageProcessingJobUpdate {
                status: "running",
                provider_task_id: Some("provider-task-succeeded"),
                provider: Some("atlas"),
                model: Some("done-model"),
                result_node_id: None,
                output_upload_id: None,
                error: None,
            },
        )
        .await
        .expect("start succeeded job");
    let succeeded_before = store
        .update_image_processing_job(
            &workspace.id,
            &succeeded.id,
            ImageProcessingJobUpdate {
                status: "succeeded",
                provider_task_id: None,
                provider: None,
                model: None,
                result_node_id: None,
                output_upload_id: Some(&upload.id),
                error: None,
            },
        )
        .await
        .expect("complete succeeded job");

    store.pool().close().await;
    let state = AppState::open_for_test(EventBus::new(16), data_dir)
        .await
        .expect("reopen");

    let queued = state
        .store
        .workspace_image_processing_job(&workspace.id, &queued.id)
        .await
        .expect("read queued");
    assert_eq!(queued.status, "interrupted");
    assert_eq!(queued.error.as_deref(), Some(RESTART_ERROR));
    assert!(queued.completed_at.is_some());
    assert!(queued.provider_task_id.is_none());
    assert!(queued.output_upload_id.is_none());

    let running = state
        .store
        .workspace_image_processing_job(&workspace.id, &running.id)
        .await
        .expect("read running");
    assert_eq!(running.status, "interrupted");
    assert_eq!(running.error.as_deref(), Some(RESTART_ERROR));
    assert!(running.completed_at.is_some());
    assert_eq!(
        running.provider_task_id.as_deref(),
        Some("provider-task-running")
    );
    assert_eq!(
        running.output_upload_id.as_deref(),
        Some(upload.id.as_str())
    );

    let succeeded = state
        .store
        .workspace_image_processing_job(&workspace.id, &succeeded.id)
        .await
        .expect("read succeeded");
    assert_eq!(succeeded.status, "succeeded");
    assert_eq!(succeeded.error, succeeded_before.error);
    assert_eq!(succeeded.completed_at, succeeded_before.completed_at);
    assert_eq!(succeeded.updated_at, succeeded_before.updated_at);
    assert_eq!(
        succeeded.provider_task_id,
        succeeded_before.provider_task_id
    );
    assert_eq!(
        succeeded.output_upload_id,
        succeeded_before.output_upload_id
    );
}

#[tokio::test]
async fn image_job_start_failure_is_failed_without_calling_provider() {
    let atlas_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind provider");
    let atlas_address = atlas_listener.local_addr().expect("provider address");
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let provider_called = tokio::spawn(async move {
        tokio::select! {
            biased;
            accepted = atlas_listener.accept() => {
                accepted.expect("accept provider");
                true
            }
            _ = stop_rx => false,
        }
    });
    let api = ImageApi::start(atlas_address).await;
    install_image_job_fault(&api.store, ImageJobFault::Start).await;

    let created = api.post_enhance().await;
    let job_id = created["job"]["id"].as_str().expect("job id");
    let job = api.wait_terminal(job_id).await;
    assert_failed_job(&job, START_ERROR);
    assert!(job["providerTaskId"].is_null());

    let record = api
        .store
        .workspace_image_processing_job(&api.workspace_id, job_id)
        .await
        .expect("durable job");
    assert_eq!(record.status, "failed");
    assert_eq!(record.error.as_deref(), Some(START_ERROR));
    assert!(record.completed_at.is_some());
    assert!(record.provider_task_id.is_none());

    tokio::time::sleep(Duration::from_millis(50)).await;
    let _ = stop_tx.send(());
    assert!(
        !provider_called.await.expect("provider watch"),
        "start failure must not call the provider"
    );
}

#[tokio::test]
async fn image_job_provider_task_save_failure_keeps_task_id_without_downloading() {
    let atlas_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind Atlas");
    let atlas_address = atlas_listener.local_addr().expect("Atlas address");
    let api = ImageApi::start(atlas_address).await;
    install_image_job_fault(&api.store, ImageJobFault::ProviderTask).await;
    let downloaded = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let downloaded_flag = Arc::clone(&downloaded);
    let atlas = tokio::spawn(async move {
        let upload_body = format!(r#"{{"url":"http://{atlas_address}/input.png"}}"#);
        serve_json(
            &atlas_listener,
            "POST /api/v1/model/uploadMedia",
            &upload_body,
        )
        .await;
        let generate_body = format!(
            r#"{{"data":{{"id":"provider-task-1","outputs":["http://{atlas_address}/output.png"]}}}}"#
        );
        serve_json(
            &atlas_listener,
            "POST /api/v1/model/generateImage",
            &generate_body,
        )
        .await;
        if tokio::time::timeout(Duration::from_millis(400), atlas_listener.accept())
            .await
            .is_ok()
        {
            downloaded_flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    });

    let created = api.post_enhance().await;
    let job_id = created["job"]["id"].as_str().expect("job id");
    let job = api.wait_terminal(job_id).await;
    assert_failed_job(&job, PROVIDER_TASK_ERROR);
    assert_eq!(job["providerTaskId"], "provider-task-1");
    assert!(job["outputUploadId"].is_null());
    atlas.await.expect("atlas script");
    assert!(
        !downloaded.load(std::sync::atomic::Ordering::SeqCst),
        "provider task save failure must not download output"
    );

    let record = api
        .store
        .workspace_image_processing_job(&api.workspace_id, job_id)
        .await
        .expect("durable job");
    assert_eq!(record.status, "failed");
    assert_eq!(record.provider_task_id.as_deref(), Some("provider-task-1"));
    assert!(record.output_upload_id.is_none());
    assert!(record.completed_at.is_some());
}

#[tokio::test]
async fn image_job_success_save_failure_keeps_upload_and_stays_failed() {
    let atlas_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind Atlas");
    let atlas_address = atlas_listener.local_addr().expect("Atlas address");
    let api = ImageApi::start(atlas_address).await;
    install_image_job_fault(&api.store, ImageJobFault::Success).await;
    let png = one_pixel_png();
    let atlas = tokio::spawn(async move {
        let upload_body = format!(r#"{{"url":"http://{atlas_address}/input.png"}}"#);
        serve_json(
            &atlas_listener,
            "POST /api/v1/model/uploadMedia",
            &upload_body,
        )
        .await;
        let generate_body = format!(
            r#"{{"data":{{"id":"provider-task-1","outputs":["http://{atlas_address}/output.png"]}}}}"#
        );
        serve_json(
            &atlas_listener,
            "POST /api/v1/model/generateImage",
            &generate_body,
        )
        .await;
        serve_png(&atlas_listener, "GET /output.png", &png).await;
    });

    let created = api.post_enhance().await;
    let job_id = created["job"]["id"].as_str().expect("job id");
    let job = api.wait_terminal(job_id).await;
    tokio::time::timeout(Duration::from_secs(5), atlas)
        .await
        .expect("Atlas finishes the output download")
        .expect("atlas script");
    assert_failed_job(&job, RESULT_ERROR);
    let output_upload_id = job["outputUploadId"].as_str().expect("stored upload");
    assert_eq!(job["providerTaskId"], "provider-task-1");

    let record = api
        .store
        .workspace_image_processing_job(&api.workspace_id, job_id)
        .await
        .expect("durable job");
    assert_eq!(record.status, "failed");
    assert_eq!(record.output_upload_id.as_deref(), Some(output_upload_id));
    assert_eq!(record.provider_task_id.as_deref(), Some("provider-task-1"));
    assert!(record.completed_at.is_some());
    api.store
        .upload(output_upload_id)
        .await
        .expect("stored file remains");

    let linked = reqwest::Client::new()
        .put(api.job_url(job_id))
        .json(&json!({
            "resultNodeId": "image_enhance",
            "outputUploadId": output_upload_id
        }))
        .send()
        .await
        .expect("link failed job");
    assert_eq!(linked.status(), reqwest::StatusCode::CONFLICT);
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

const START_ERROR: &str = "image processing job could not start";
const PROVIDER_TASK_ERROR: &str = "image processing job could not save its provider task";
const RESULT_ERROR: &str = "image processing job could not save its result";
const RESTART_ERROR: &str = "image processing job was interrupted by a server restart";

struct ImageApi {
    server: tokio::task::JoinHandle<()>,
    store: Store,
    workspace_id: String,
    base: String,
    _directory: tempfile::TempDir,
}

impl Drop for ImageApi {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl ImageApi {
    async fn start(atlas_address: std::net::SocketAddr) -> Self {
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
        let provider = RuntimeProvider::Atlas(
            AtlasProvider::new(ApiProviderConfig::atlas(
                "test-key".to_owned(),
                format!("http://{atlas_address}/v1"),
            ))
            .with_loopback_output_http(),
        );
        let state = AppState::with_store_agent_provider(
            EventBus::new(16),
            store.clone(),
            data_dir.clone(),
            Arc::new(FailingWorkbenchAgent),
            data_dir.join("sessions"),
            provider,
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind Helixflow");
        let address = listener.local_addr().expect("Helixflow address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app(state)).await.expect("serve");
        });
        Self {
            server,
            store,
            workspace_id: workspace.id,
            base: format!("http://{address}"),
            _directory: directory,
        }
    }

    fn jobs_url(&self) -> String {
        format!(
            "{}/api/workspaces/{}/image-processing-jobs",
            self.base, self.workspace_id
        )
    }

    fn job_url(&self, job_id: &str) -> String {
        format!("{}/{}", self.jobs_url(), job_id)
    }

    async fn post_enhance(&self) -> Value {
        let response = reqwest::Client::new()
            .post(self.jobs_url())
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
        response.json().await.expect("execution response")
    }

    async fn wait_terminal(&self, job_id: &str) -> Value {
        let client = reqwest::Client::new();
        let url = self.job_url(job_id);
        for _ in 0..100 {
            let response = client.get(&url).send().await.expect("read job");
            let job: Value = response.json().await.expect("job response");
            if !matches!(job["status"].as_str(), Some("queued" | "running")) {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("image job {job_id} did not reach a terminal status");
    }
}

enum ImageJobFault {
    Start,
    ProviderTask,
    Success,
}

async fn install_image_job_fault(store: &Store, fault: ImageJobFault) {
    let sql = match fault {
        ImageJobFault::Start => {
            "CREATE TRIGGER image_processing_job_test_fault
             BEFORE UPDATE ON image_processing_jobs
             FOR EACH ROW
             WHEN OLD.status = 'queued' AND NEW.status = 'running'
             BEGIN
                 SELECT RAISE(ABORT, 'image processing job test fault');
             END"
        }
        ImageJobFault::ProviderTask => {
            "CREATE TRIGGER image_processing_job_test_fault
             BEFORE UPDATE ON image_processing_jobs
             FOR EACH ROW
             WHEN OLD.status = 'running'
               AND NEW.status = 'running'
               AND OLD.provider_task_id IS NULL
               AND NEW.provider_task_id IS NOT NULL
             BEGIN
                 SELECT RAISE(ABORT, 'image processing job test fault');
             END"
        }
        ImageJobFault::Success => {
            "CREATE TRIGGER image_processing_job_test_fault
             BEFORE UPDATE ON image_processing_jobs
             FOR EACH ROW
             WHEN NEW.status = 'succeeded'
             BEGIN
                 SELECT RAISE(ABORT, 'image processing job test fault');
             END"
        }
    };
    sqlx::query(sql)
        .execute(store.pool())
        .await
        .expect("install image job update fault");
}

fn assert_failed_job(job: &Value, error: &str) {
    assert_eq!(job["status"], "failed");
    assert_eq!(job["error"], error);
    assert!(!job["completedAt"].as_str().unwrap_or_default().is_empty());
}

async fn serve_json(listener: &tokio::net::TcpListener, expected_prefix: &str, body: &str) {
    let (request, stream) = read_request(listener).await;
    assert!(
        request.starts_with(expected_prefix),
        "expected {expected_prefix}, got {}",
        request.lines().next().unwrap_or_default()
    );
    respond_json(stream, body).await;
}

async fn serve_png(listener: &tokio::net::TcpListener, expected_prefix: &str, body: &[u8]) {
    let (request, stream) = read_request(listener).await;
    assert!(
        request.starts_with(expected_prefix),
        "expected {expected_prefix}, got {}",
        request.lines().next().unwrap_or_default()
    );
    respond_bytes(stream, "image/png", body).await;
}
