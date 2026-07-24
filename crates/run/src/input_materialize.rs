use std::collections::BTreeMap;
use std::path::Path;

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
