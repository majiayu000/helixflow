use std::path::{Component, Path as FsPath, PathBuf};

use axum::{
    Json,
    body::Body,
    extract::{Multipart, Path, State},
    http::{HeaderValue, header},
    response::Response,
};
use helixflow_store::NewUpload;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::api_error::ApiError;
use crate::app_state::AppState;

const DEFAULT_MAX_UPLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_CONFIGURED_UPLOAD_BYTES: usize = 1024 * 1024 * 1024;

/// Maximum accepted upload size (16 MiB by default).
fn max_upload_bytes() -> usize {
    parse_max_upload_bytes(std::env::var("HELIXFLOW_MAX_UPLOAD_BYTES").ok().as_deref())
        .unwrap_or(DEFAULT_MAX_UPLOAD_BYTES)
}

pub(crate) fn validate_upload_config() -> Result<(), String> {
    match std::env::var("HELIXFLOW_MAX_UPLOAD_BYTES") {
        Ok(value) => parse_max_upload_bytes(Some(&value)).map(|_| ()),
        Err(std::env::VarError::NotPresent) => Ok(()),
        Err(std::env::VarError::NotUnicode(_)) => Err(upload_limit_error()),
    }
}

fn parse_max_upload_bytes(raw: Option<&str>) -> Result<usize, String> {
    let Some(raw) = raw else {
        return Ok(DEFAULT_MAX_UPLOAD_BYTES);
    };
    let value = raw.parse::<usize>().map_err(|_| upload_limit_error())?;
    if value == 0 || value > MAX_CONFIGURED_UPLOAD_BYTES {
        return Err(upload_limit_error());
    }
    Ok(value)
}

fn upload_limit_error() -> String {
    format!("HELIXFLOW_MAX_UPLOAD_BYTES must be an integer from 1 to {MAX_CONFIGURED_UPLOAD_BYTES}")
}

/// Leave bounded room for multipart headers while keeping the request body
/// itself capped before the handler reads any field data.
pub(crate) fn upload_request_limit() -> usize {
    max_upload_bytes().saturating_add(64 * 1024)
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn upload_limit_is_bounded_and_fails_closed() {
        assert_eq!(
            parse_max_upload_bytes(None).expect("default"),
            DEFAULT_MAX_UPLOAD_BYTES
        );
        assert_eq!(parse_max_upload_bytes(Some("1024")).expect("one KiB"), 1024);
        for invalid in ["", "0", "1073741825", "not-a-number"] {
            assert!(parse_max_upload_bytes(Some(invalid)).is_err());
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UploadResponse {
    pub(crate) id: String,
    pub(crate) storage_uri: String,
    pub(crate) filename: String,
    pub(crate) mime: String,
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
    while let Some(mut field) = multipart
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
        let limit = max_upload_bytes();
        let mut bytes = Vec::new();
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|err| ApiError::bad_request(format!("read upload body: {err}")))?
        {
            let next_len = bytes
                .len()
                .checked_add(chunk.len())
                .ok_or_else(|| ApiError::bad_request("uploaded file size overflow"))?;
            if next_len > limit {
                return Err(ApiError::bad_request(format!(
                    "uploaded file exceeds the {limit} byte limit"
                )));
            }
            bytes.extend_from_slice(&chunk);
        }
        uploaded = Some(persist_workspace_upload(&state, &workspace_id, &filename, &bytes).await?);
        break;
    }

    let uploaded =
        uploaded.ok_or_else(|| ApiError::bad_request("multipart field `file` is required"))?;
    Ok(Json(json!(uploaded)))
}

pub(crate) async fn persist_workspace_upload(
    state: &AppState,
    workspace_id: &str,
    filename: &str,
    bytes: &[u8],
) -> Result<UploadResponse, ApiError> {
    if bytes.is_empty() {
        return Err(ApiError::bad_request("uploaded file is empty"));
    }
    let limit = max_upload_bytes();
    if bytes.len() > limit {
        return Err(ApiError::bad_request(format!(
            "uploaded file exceeds the {limit} byte limit"
        )));
    }
    let (mime, extension) = detect_media_type(bytes).ok_or_else(|| {
        ApiError::bad_request(
            "uploaded file is not a supported image, video, or audio (png, jpeg, webp, mp4, webm, wav, mp3)",
        )
    })?;
    let upload_uuid = uuid::Uuid::now_v7();
    let relative = PathBuf::from("uploads")
        .join(workspace_id)
        .join(format!("{upload_uuid}.{extension}"));
    let full_path = state.data_dir.join(&relative);
    let parent = full_path
        .parent()
        .ok_or_else(|| ApiError::server_error("upload path has no parent"))?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|err| ApiError::io("create upload directory", err))?;
    let tmp_path = full_path.with_extension("tmp");
    tokio::fs::write(&tmp_path, bytes)
        .await
        .map_err(|err| ApiError::io("write upload temp file", err))?;
    if let Err(commit_error) = tokio::fs::rename(&tmp_path, &full_path).await {
        let cleanup_error = match tokio::fs::remove_file(&tmp_path).await {
            Ok(()) => None,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => Some(error),
        };
        return Err(match cleanup_error {
            Some(cleanup_error) => ApiError::server_error(format!(
                "commit upload file failed: {commit_error}; temporary upload cleanup failed: {cleanup_error}"
            )),
            None => ApiError::io("commit upload file", commit_error),
        });
    }

    let sha256 = format!("sha256:{}", hex::encode(Sha256::digest(bytes)));
    let relative_string = relative.to_string_lossy().into_owned();
    let record = match state
        .store
        .create_upload(NewUpload {
            workspace_id,
            filename,
            file_path: &relative_string,
            sha256: &sha256,
            mime: Some(mime),
        })
        .await
    {
        Ok(record) => record,
        Err(store_error) => {
            let cleanup_error = match tokio::fs::remove_file(&full_path).await {
                Ok(()) => None,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => Some(error),
            };
            return Err(match cleanup_error {
                Some(cleanup_error) => ApiError::server_error(format!(
                    "upload record commit failed: {store_error}; published upload cleanup failed: {cleanup_error}"
                )),
                None => ApiError::store(store_error),
            });
        }
    };
    Ok(UploadResponse {
        storage_uri: format!("upload://{}", record.id),
        id: record.id,
        filename: filename.to_owned(),
        mime: mime.to_owned(),
    })
}

pub(crate) async fn download_workspace_upload(
    Path((workspace_id, upload_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Response<Body>, ApiError> {
    let upload = match state
        .store
        .workspace_upload(&workspace_id, &upload_id)
        .await
    {
        Ok(record) => record,
        Err(err) if err.is_not_found() => return Err(ApiError::not_found("upload was not found")),
        Err(err) => return Err(ApiError::store(err)),
    };
    let path = upload_content_path(&state.data_dir, &workspace_id, &upload.file_path)?;
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(ApiError::not_found("upload content was not found"));
        }
        Err(err) => return Err(ApiError::io("read upload content", err)),
    };
    let mut response = Response::new(Body::from(bytes));
    if let Some(mime) = upload.mime.as_deref()
        && let Ok(value) = HeaderValue::from_str(mime)
    {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    Ok(response)
}

fn upload_content_path(
    data_dir: &FsPath,
    workspace_id: &str,
    file_path: &str,
) -> Result<PathBuf, ApiError> {
    let relative = FsPath::new(file_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ApiError::not_found("upload content was not found"));
    }
    let mut components = relative.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(prefix)), Some(Component::Normal(workspace)))
            if prefix == "uploads" && workspace == workspace_id => {}
        _ => return Err(ApiError::not_found("upload content was not found")),
    }
    Ok(data_dir.join(relative))
}

fn detect_media_type(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
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
    if bytes.len() > 12 && bytes[4..8] == *b"ftyp" {
        return Some(("video/mp4", "mp4"));
    }
    if bytes.len() > 4 && bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some(("video/webm", "webm"));
    }
    if bytes.len() > 12 && bytes.starts_with(b"RIFF") && bytes[8..12] == *b"WAVE" {
        return Some(("audio/wav", "wav"));
    }
    if bytes.len() > 3 && bytes.starts_with(b"ID3") {
        return Some(("audio/mpeg", "mp3"));
    }
    if bytes.len() > 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0 {
        return Some(("audio/mpeg", "mp3"));
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
        assert_eq!(detect_media_type(&png), Some(("image/png", "png")));
        let mut jpg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpg.extend_from_slice(&[0; 8]);
        assert_eq!(detect_media_type(&jpg), Some(("image/jpeg", "jpg")));
        assert_eq!(detect_media_type(b"not an image"), None);
        let mut mp4 = vec![0, 0, 0, 24];
        mp4.extend_from_slice(b"ftypisom");
        mp4.extend_from_slice(&[0; 8]);
        assert_eq!(detect_media_type(&mp4), Some(("video/mp4", "mp4")));
        let mut wav = Vec::from(*b"RIFF");
        wav.extend_from_slice(&[0; 4]);
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(&[0; 8]);
        assert_eq!(detect_media_type(&wav), Some(("audio/wav", "wav")));
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

    fn valid_png_with_size(size: usize) -> Vec<u8> {
        let mut png = valid_png();
        png.resize(size, 0);
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

        let content = reqwest::Client::new()
            .get(format!(
                "http://{addr}/api/workspaces/{workspace_id}/uploads/{upload_id}/content"
            ))
            .send()
            .await
            .expect("download request");
        assert_eq!(content.status(), 200);
        assert_eq!(content.headers().get("content-type").unwrap(), "image/png");
        assert_eq!(content.bytes().await.expect("bytes").as_ref(), valid_png());
    }

    #[tokio::test]
    async fn upload_content_is_scoped_to_workspace() {
        let (_dir, state, addr, workspace_id) = serve_app().await;
        let (content_type, body) = multipart_body(&valid_png(), "photo.png");
        let payload: serde_json::Value = reqwest::Client::new()
            .post(format!(
                "http://{addr}/api/workspaces/{workspace_id}/uploads"
            ))
            .header("content-type", content_type)
            .body(body)
            .send()
            .await
            .expect("upload request")
            .json()
            .await
            .expect("upload json");
        let upload_id = payload["storageUri"]
            .as_str()
            .expect("storageUri")
            .trim_start_matches("upload://");
        let other = state
            .store
            .create_workspace("Other")
            .await
            .expect("other workspace");

        let response = reqwest::Client::new()
            .get(format!(
                "http://{addr}/api/workspaces/{}/uploads/{upload_id}/content",
                other.id
            ))
            .send()
            .await
            .expect("cross-workspace download");
        assert_eq!(response.status(), 404);
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

    #[tokio::test]
    async fn upload_accepts_image_larger_than_axum_default_body_limit() {
        let (_dir, _state, addr, workspace_id) = serve_app().await;
        let (content_type, body) =
            multipart_body(&valid_png_with_size(3 * 1024 * 1024), "large.png");

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
    }

    #[tokio::test]
    async fn upload_store_failure_removes_published_file() {
        let (_dir, state, addr, workspace_id) = serve_app().await;
        sqlx::query(
            r#"
            CREATE TRIGGER reject_upload_insert
            BEFORE INSERT ON uploads
            BEGIN
              SELECT RAISE(ABORT, 'forced upload store failure');
            END
            "#,
        )
        .execute(state.store.pool())
        .await
        .expect("install failure trigger");
        let (content_type, body) = multipart_body(&valid_png(), "orphan.png");

        let response = reqwest::Client::new()
            .post(format!(
                "http://{addr}/api/workspaces/{workspace_id}/uploads"
            ))
            .header("content-type", content_type)
            .body(body)
            .send()
            .await
            .expect("upload request");

        assert_eq!(response.status(), 500);
        let workspace_upload_dir = state.data_dir.join("uploads").join(&workspace_id);
        let entries = std::fs::read_dir(workspace_upload_dir)
            .expect("upload directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("read upload directory");
        assert!(
            entries.is_empty(),
            "store failure must not leave upload bytes"
        );
    }
}
