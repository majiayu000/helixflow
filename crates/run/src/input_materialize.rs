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

/// Convert a wired image artifact into an Atlas-compatible public URL or
/// bounded data URL. Only persisted image artifacts from the local artifact
/// and upload roots are read.
pub(crate) async fn materialize_image_input(
    store: &Store,
    artifact_root: &Path,
    inputs: &BTreeMap<String, ArtifactRef>,
    port: &str,
) -> RunResult<String> {
    const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
    let input = inputs.get(port).ok_or_else(|| RunError::MissingInput {
        node_id: String::new(),
        port: port.to_owned(),
    })?;
    let artifact = store.artifact(&input.artifact_id).await?;
    if artifact.kind != "image" {
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
        return Err(RunError::ArtifactPersistence(
            "wired image input is not a safe local artifact path".to_owned(),
        ));
    }
    let mime = artifact
        .mime
        .as_deref()
        .filter(|mime| matches!(*mime, "image/png" | "image/jpeg" | "image/webp"))
        .ok_or_else(|| {
            RunError::ArtifactPersistence("wired image MIME is unsupported".to_owned())
        })?;
    let bytes = tokio::fs::read(artifact_root.join(&artifact.storage_uri))
        .await
        .map_err(|error| {
            RunError::ArtifactPersistence(format!("read wired image input: {error}"))
        })?;
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        return Err(RunError::ArtifactPersistence(
            "wired image must be between 1 byte and 20 MiB".to_owned(),
        ));
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
