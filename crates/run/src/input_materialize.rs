use std::collections::BTreeMap;
use std::path::Path;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use helixflow_gateway::ArtifactRef;
use helixflow_store::Store;

use crate::cache::is_local_artifact_path;
use crate::{RunError, RunResult};

/// Materialize upstream text artifacts so providers can consume wired inputs
/// instead of hand-typed params (HF-003). Text lives either in the artifact
/// meta (`input.text` nodes) or in a locally persisted file (provider text
/// outputs such as prompt_writer).
pub(crate) async fn materialize_text_inputs(
    store: &Store,
    artifact_root: &Path,
    inputs: &BTreeMap<String, ArtifactRef>,
) -> RunResult<BTreeMap<String, String>> {
    let mut texts = BTreeMap::new();
    for (port, input) in inputs {
        let artifact = store.artifact(&input.artifact_id).await?;
        if artifact.kind != "text" {
            continue;
        }
        if let Some(meta_json) = artifact.meta_json.as_deref()
            && let Ok(meta) = serde_json::from_str::<serde_json::Value>(meta_json)
            && let Some(text) = meta.get("text").and_then(serde_json::Value::as_str)
        {
            texts.insert(port.clone(), text.to_owned());
            continue;
        }
        if is_local_artifact_path(&artifact.storage_uri) {
            let path = artifact_root.join(&artifact.storage_uri);
            let bytes = tokio::fs::read(&path).await.map_err(|err| {
                RunError::ArtifactPersistence(format!(
                    "read wired text input `{}`: {err}",
                    artifact.storage_uri
                ))
            })?;
            let text = String::from_utf8(bytes).map_err(|_| {
                RunError::ArtifactPersistence(format!(
                    "wired text input `{}` is not valid UTF-8",
                    artifact.storage_uri
                ))
            })?;
            texts.insert(port.clone(), text);
            continue;
        }
        // A wired text input that can neither be read from meta nor from
        // local storage must fail the step instead of silently degrading to
        // the hand-typed param (HF-003 / U-29).
        return Err(RunError::MissingInput {
            node_id: artifact.node_id.unwrap_or_default(),
            port: port.clone(),
        });
    }
    Ok(texts)
}

/// Convert wired image artifacts into Atlas-compatible public URLs or
/// bounded data URLs. `port` is the first edge; `port#1`, `port#2`, … are
/// extra list edges. Only persisted image artifacts from the local artifact
/// and upload roots are read.
pub(crate) async fn materialize_image_inputs(
    store: &Store,
    artifact_root: &Path,
    inputs: &BTreeMap<String, ArtifactRef>,
    port: &str,
) -> RunResult<Vec<String>> {
    materialize_media_inputs(
        store,
        artifact_root,
        inputs,
        port,
        "image",
        true,
        &["image/png", "image/jpeg", "image/webp"],
        20 * 1024 * 1024,
    )
    .await
}

pub(crate) async fn materialize_video_inputs(
    store: &Store,
    artifact_root: &Path,
    inputs: &BTreeMap<String, ArtifactRef>,
    port: &str,
) -> RunResult<Vec<String>> {
    materialize_media_inputs(
        store,
        artifact_root,
        inputs,
        port,
        "video",
        false,
        &["video/mp4", "video/webm"],
        50 * 1024 * 1024,
    )
    .await
}

pub(crate) async fn materialize_audio_inputs(
    store: &Store,
    artifact_root: &Path,
    inputs: &BTreeMap<String, ArtifactRef>,
    port: &str,
) -> RunResult<Vec<String>> {
    materialize_media_inputs(
        store,
        artifact_root,
        inputs,
        port,
        "audio",
        false,
        &["audio/wav", "audio/wave", "audio/mpeg", "audio/mp3"],
        15 * 1024 * 1024,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn materialize_media_inputs(
    store: &Store,
    artifact_root: &Path,
    inputs: &BTreeMap<String, ArtifactRef>,
    port: &str,
    kind: &str,
    required: bool,
    allowed_mimes: &[&str],
    max_bytes: usize,
) -> RunResult<Vec<String>> {
    let mut keys = Vec::new();
    if inputs.contains_key(port) {
        keys.push(port.to_owned());
    }
    let mut index = 1;
    while inputs.contains_key(&format!("{port}#{index}")) {
        keys.push(format!("{port}#{index}"));
        index += 1;
    }
    if keys.is_empty() {
        if required {
            return Err(RunError::MissingInput {
                node_id: String::new(),
                port: port.to_owned(),
            });
        }
        return Ok(Vec::new());
    }
    let mut media = Vec::with_capacity(keys.len());
    for key in keys {
        media.push(
            materialize_one_media(
                store,
                artifact_root,
                inputs,
                &key,
                kind,
                allowed_mimes,
                max_bytes,
            )
            .await?,
        );
    }
    Ok(media)
}

async fn materialize_one_media(
    store: &Store,
    artifact_root: &Path,
    inputs: &BTreeMap<String, ArtifactRef>,
    port: &str,
    kind: &str,
    allowed_mimes: &[&str],
    max_bytes: usize,
) -> RunResult<String> {
    let input = inputs.get(port).ok_or_else(|| RunError::MissingInput {
        node_id: String::new(),
        port: port.to_owned(),
    })?;
    let artifact = store.artifact(&input.artifact_id).await?;
    if artifact.kind != kind {
        return Err(RunError::MissingInput {
            node_id: artifact.node_id.unwrap_or_default(),
            port: port.to_owned(),
        });
    }
    if artifact.storage_uri.starts_with("https://") {
        crate::artifact_remote::parse_remote_url(&artifact.storage_uri)?;
        return Ok(artifact.storage_uri);
    }
    if !is_safe_local_media_path(&artifact.storage_uri) {
        return Err(RunError::ArtifactPersistence(format!(
            "wired {kind} input is not a safe local artifact path"
        )));
    }
    let mime = artifact
        .mime
        .as_deref()
        .filter(|mime| allowed_mimes.contains(mime))
        .ok_or_else(|| {
            RunError::ArtifactPersistence(format!("wired {kind} MIME is unsupported"))
        })?;
    let bytes = tokio::fs::read(artifact_root.join(&artifact.storage_uri))
        .await
        .map_err(|error| {
            RunError::ArtifactPersistence(format!("read wired {kind} input: {error}"))
        })?;
    if bytes.is_empty() || bytes.len() > max_bytes {
        return Err(RunError::ArtifactPersistence(format!(
            "wired {kind} must be between 1 byte and {max_bytes} bytes"
        )));
    }
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

fn is_safe_local_media_path(value: &str) -> bool {
    let path = Path::new(value);
    let mut components = path.components();
    let Some(std::path::Component::Normal(root)) = components.next() else {
        return false;
    };
    matches!(root.to_str(), Some("artifacts" | "uploads"))
        && components.all(|component| matches!(component, std::path::Component::Normal(_)))
}
