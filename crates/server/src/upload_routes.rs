use std::path::PathBuf;

use axum::{
    Json,
    extract::{Multipart, Path, State},
};
use helixflow_store::NewUpload;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::api_error::ApiError;
use crate::app_state::AppState;

/// Maximum accepted upload size (16 MiB by default).
fn max_upload_bytes() -> usize {
    std::env::var("HELIXFLOW_MAX_UPLOAD_BYTES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(16 * 1024 * 1024)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UploadResponse {
    id: String,
    storage_uri: String,
    filename: String,
    mime: String,
}

/// Accept a local image upload, persist it under the data dir, and register
/// it so `input.image` nodes can reference it via `upload://{id}` (HF-008).
pub(crate) async fn upload_workspace_image(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, ApiError> {
    state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;

    let mut uploaded: Option<UploadResponse> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| ApiError::bad_request(format!("invalid multipart request: {err}")))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let filename = field
            .file_name()
            .map(str::to_owned)
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "upload".to_owned());
        let bytes = field
            .bytes()
            .await
            .map_err(|err| ApiError::bad_request(format!("read upload body: {err}")))?;
        if bytes.is_empty() {
            return Err(ApiError::bad_request("uploaded file is empty"));
        }
        if bytes.len() > max_upload_bytes() {
            return Err(ApiError::bad_request(format!(
                "uploaded file exceeds the {} byte limit",
                max_upload_bytes()
            )));
        }
        let (mime, extension) = detect_image_type(&bytes).ok_or_else(|| {
            ApiError::bad_request("uploaded file is not a supported image (png, jpeg, webp)")
        })?;

        let upload_uuid = uuid::Uuid::now_v7();
        let relative = PathBuf::from("uploads")
            .join(&workspace_id)
            .join(format!("{upload_uuid}.{extension}"));
        let full_path = state.data_dir.join(&relative);
        let parent = full_path
            .parent()
            .ok_or_else(|| ApiError::server_error("upload path has no parent"))?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| ApiError::io("create upload directory", err))?;
        let tmp_path = full_path.with_extension("tmp");
        tokio::fs::write(&tmp_path, &bytes)
            .await
            .map_err(|err| ApiError::io("write upload temp file", err))?;
        tokio::fs::rename(&tmp_path, &full_path)
            .await
            .map_err(|err| ApiError::io("commit upload file", err))?;

        let sha256 = format!("sha256:{:x}", Sha256::digest(&bytes));
        let relative_string = relative.to_string_lossy().into_owned();
        let record = state
            .store
            .create_upload(NewUpload {
                workspace_id: &workspace_id,
                filename: &filename,
                file_path: &relative_string,
                sha256: &sha256,
                mime: Some(mime),
            })
            .await
            .map_err(ApiError::store)?;
        uploaded = Some(UploadResponse {
            storage_uri: format!("upload://{}", record.id),
            id: record.id,
            filename,
            mime: mime.to_owned(),
        });
        break;
    }

    let uploaded =
        uploaded.ok_or_else(|| ApiError::bad_request("multipart field `file` is required"))?;
    Ok(Json(json!(uploaded)))
}

fn detect_image_type(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.len() > 16
        && bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
        && bytes[12..16] == *b"IHDR"
    {
        return Some(("image/png", "png"));
    }
    if bytes.len() > 3 && bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(("image/jpeg", "jpg"));
    }
    if bytes.len() > 12 && bytes.starts_with(b"RIFF") && bytes[8..12] == *b"WEBP" {
        return Some(("image/webp", "webp"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_supported_image_types() {
        let mut png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&[0, 0, 0, 13]);
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&[0; 8]);
        assert_eq!(detect_image_type(&png), Some(("image/png", "png")));
        let mut jpg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpg.extend_from_slice(&[0; 8]);
        assert_eq!(detect_image_type(&jpg), Some(("image/jpeg", "jpg")));
        assert_eq!(detect_image_type(b"not an image"), None);
    }
}

#[cfg(test)]
mod route_tests {
    use std::sync::Arc;

    use helixflow_run::EventBus;
    use helixflow_store::Store;

    use crate::app_state::AppState;
    use crate::test_support::FailingWorkbenchAgent;

    fn valid_png() -> Vec<u8> {
        let mut png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&[0x00, 0x00, 0x00, 0x0D]);
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&[0; 16]);
        png
    }

    async fn serve_app() -> (tempfile::TempDir, AppState, std::net::SocketAddr, String) {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store.create_workspace("Uploads").await.expect("workspace");
        let state = AppState::with_store_agent(
            EventBus::new(16),
            store,
            data_dir.clone(),
            Arc::new(FailingWorkbenchAgent),
            data_dir.join("sessions"),
        );
        let app = crate::app(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        (dir, state, addr, workspace.id)
    }

    fn multipart_body(bytes: &[u8], filename: &str) -> (String, Vec<u8>) {
        let boundary = "hf008boundary";
        let mut body = Vec::new();
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; \
                 filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        (format!("multipart/form-data; boundary={boundary}"), body)
    }

    #[tokio::test]
    async fn upload_persists_image_and_returns_upload_uri() {
        let (_dir, state, addr, workspace_id) = serve_app().await;
        let (content_type, body) = multipart_body(&valid_png(), "photo.png");

        let response = reqwest::Client::new()
            .post(format!(
                "http://{addr}/api/workspaces/{workspace_id}/uploads"
            ))
            .header("content-type", content_type)
            .body(body)
            .send()
            .await
            .expect("upload request");
        assert_eq!(response.status(), 200);
        let payload: serde_json::Value = response.json().await.expect("upload json");
        let storage_uri = payload["storageUri"].as_str().expect("storageUri");
        assert!(storage_uri.starts_with("upload://"));
        assert_eq!(payload["mime"], "image/png");

        let upload_id = storage_uri.trim_start_matches("upload://");
        let record = state.store.upload(upload_id).await.expect("upload record");
        assert!(state.data_dir.join(&record.file_path).exists());
        assert!(record.sha256.starts_with("sha256:"));
    }

    #[tokio::test]
    async fn non_image_upload_is_rejected() {
        let (_dir, _state, addr, workspace_id) = serve_app().await;
        let (content_type, body) = multipart_body(b"plain text, not an image", "note.txt");

        let response = reqwest::Client::new()
            .post(format!(
                "http://{addr}/api/workspaces/{workspace_id}/uploads"
            ))
            .header("content-type", content_type)
            .body(body)
            .send()
            .await
            .expect("upload request");
        assert_eq!(response.status(), 400);
    }
}
