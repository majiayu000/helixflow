use std::path::{Path, PathBuf};

use helixflow_gateway::{ArtifactContent, ArtifactKind, ArtifactPayload};

use crate::{RunError, RunResult};

pub(crate) fn default_artifact_root() -> PathBuf {
    std::env::temp_dir().join("helixflow-run-artifacts")
}

pub(crate) async fn persist_provider_artifact(
    root: &Path,
    run_id: &str,
    step_id: &str,
    node_id: &str,
    payload: &ArtifactPayload,
) -> RunResult<String> {
    match &payload.content {
        ArtifactContent::InlineBytes { bytes, ext_hint } => {
            let extension = extension_for_payload(payload, ext_hint.as_deref());
            let relative = artifact_relative_path(run_id, step_id, node_id, &extension);
            write_artifact_file(root, &relative, bytes).await?;
            Ok(relative.to_string_lossy().into_owned())
        }
        ArtifactContent::RemoteUrl { url } => {
            let parsed = reqwest::Url::parse(url).map_err(|_| {
                RunError::ArtifactPersistence("remote artifact URL is invalid".to_owned())
            })?;
            if parsed.scheme() != "https" {
                return Err(RunError::ArtifactPersistence(
                    "remote artifact URL must use https".to_owned(),
                ));
            }
            let response = reqwest::get(parsed).await.map_err(|err| {
                RunError::ArtifactPersistence(format!("remote artifact download failed: {err}"))
            })?;
            if !response.status().is_success() {
                return Err(RunError::ArtifactPersistence(format!(
                    "remote artifact download returned HTTP {}",
                    response.status().as_u16()
                )));
            }
            let bytes = response.bytes().await.map_err(|err| {
                RunError::ArtifactPersistence(format!("remote artifact body failed: {err}"))
            })?;
            let extension = extension_for_payload(payload, None);
            let relative = artifact_relative_path(run_id, step_id, node_id, &extension);
            write_artifact_file(root, &relative, &bytes).await?;
            Ok(relative.to_string_lossy().into_owned())
        }
        ArtifactContent::None => Ok(payload.storage_uri.clone()),
    }
}

async fn write_artifact_file(root: &Path, relative: &Path, bytes: &[u8]) -> RunResult<()> {
    let full_path = root.join(relative);
    let parent = full_path
        .parent()
        .ok_or_else(|| RunError::ArtifactPersistence("artifact path has no parent".to_owned()))?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|err| RunError::ArtifactPersistence(err.to_string()))?;
    tokio::fs::write(&full_path, bytes)
        .await
        .map_err(|err| RunError::ArtifactPersistence(err.to_string()))?;
    Ok(())
}

fn artifact_relative_path(run_id: &str, step_id: &str, node_id: &str, extension: &str) -> PathBuf {
    PathBuf::from("artifacts")
        .join(run_id)
        .join(format!("{step_id}-{node_id}.{extension}"))
}

fn extension_for_payload(payload: &ArtifactPayload, ext_hint: Option<&str>) -> String {
    if let Some(extension) = ext_hint.and_then(safe_extension) {
        return extension;
    }
    mime_extension(&payload.mime).unwrap_or_else(|| match payload.kind {
        ArtifactKind::Text => "txt".to_owned(),
        ArtifactKind::Image => "bin".to_owned(),
        ArtifactKind::Video => "bin".to_owned(),
        ArtifactKind::Json => "json".to_owned(),
    })
}

fn safe_extension(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_start_matches('.');
    (!trimmed.is_empty()
        && trimmed.len() <= 12
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-'))
    .then(|| trimmed.to_ascii_lowercase())
}

fn mime_extension(mime_value: &str) -> Option<String> {
    let parsed = mime_value.parse::<mime::Mime>().ok()?;
    if parsed.type_() == mime::IMAGE && parsed.subtype() == mime::PNG {
        return Some("png".to_owned());
    }
    if parsed.type_() == mime::IMAGE && parsed.subtype() == mime::JPEG {
        return Some("jpg".to_owned());
    }
    if parsed.type_() == mime::VIDEO && parsed.subtype() == mime::MP4 {
        return Some("mp4".to_owned());
    }
    if parsed.type_() == mime::TEXT {
        return Some("txt".to_owned());
    }
    if parsed == mime::APPLICATION_JSON {
        return Some("json".to_owned());
    }
    None
}
