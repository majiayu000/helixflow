use std::collections::BTreeMap;
use std::path::Path;

use helixflow_gateway::{ArtifactRef, Provider};
use helixflow_graph::ExecutionStep;
use helixflow_store::{ArtifactRecord, NewArtifact, NewNodeCacheEntry, RunStepRecord};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{RunError, RunResult, RunService, RunStepState, StepOutput};

#[derive(Debug, Clone)]
pub(crate) struct StepCacheKey {
    provider: String,
    cache_key: String,
    input_hash_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CachedArtifactLink {
    pub(crate) port: Option<String>,
    pub(crate) artifact_id: String,
}

impl<P> RunService<P>
where
    P: Provider + Clone + Send + Sync + 'static,
{
    pub(crate) async fn try_cache_hit(
        &self,
        workspace_id: &str,
        run_id: &str,
        step: &ExecutionStep,
        record: &RunStepRecord,
        cache_key: &StepCacheKey,
    ) -> RunResult<Option<Vec<StepOutput>>> {
        let Some(entry) = self
            .store
            .node_cache_entry(
                workspace_id,
                &cache_key.provider,
                &step.node_type,
                &step.node_id,
                &cache_key.cache_key,
            )
            .await?
        else {
            return Ok(None);
        };
        let links: Vec<CachedArtifactLink> = serde_json::from_str(&entry.artifact_ids_json)?;
        if links.is_empty() {
            return Ok(None);
        }

        let mut sources = Vec::new();
        for link in &links {
            let source = match self.store.artifact(&link.artifact_id).await {
                Ok(source) => source,
                Err(err) if err.is_not_found() => return Ok(None),
                Err(err) => return Err(err.into()),
            };
            if source.workspace_id != workspace_id
                || source.review_state == "rejected"
                || !self.cached_artifact_available(&source).await?
            {
                return Ok(None);
            }
            sources.push((link.port.clone(), source));
        }

        let mut outputs = Vec::new();
        for (port, source) in sources {
            let artifact = self
                .copy_cached_artifact(workspace_id, run_id, &record.id, &step.node_id, &source)
                .await?;
            if let Some(port) = port {
                outputs.push(StepOutput {
                    port,
                    artifact: ArtifactRef {
                        artifact_id: artifact.id,
                        storage_uri: artifact.storage_uri,
                    },
                });
            }
        }

        self.store.touch_node_cache_entry(&entry.id).await?;
        let metadata = serde_json::to_string(&json!({
            "cached": true,
            "cacheKey": cache_key.cache_key,
        }))?;
        self.store
            .mark_run_step_cached_succeeded(&record.id, &metadata)
            .await?;
        self.emit_node_state_cached(workspace_id, run_id, step, true)
            .await?;
        Ok(Some(outputs))
    }

    pub(crate) async fn refresh_step_cache(
        &self,
        workspace_id: &str,
        step: &ExecutionStep,
        cache_key: &StepCacheKey,
        links: &[CachedArtifactLink],
    ) -> RunResult<()> {
        let artifact_ids_json = serde_json::to_string(links)?;
        self.store
            .upsert_node_cache_entry(NewNodeCacheEntry {
                workspace_id,
                provider: &cache_key.provider,
                node_type: &step.node_type,
                node_id: &step.node_id,
                cache_key: &cache_key.cache_key,
                input_hash_json: &cache_key.input_hash_json,
                artifact_ids_json: &artifact_ids_json,
            })
            .await?;
        Ok(())
    }

    pub(crate) async fn copy_cached_artifact(
        &self,
        workspace_id: &str,
        run_id: &str,
        step_id: &str,
        node_id: &str,
        source: &ArtifactRecord,
    ) -> RunResult<ArtifactRecord> {
        Ok(self
            .store
            .create_artifact(NewArtifact {
                workspace_id,
                run_id: Some(run_id),
                run_step_id: Some(step_id),
                node_id: Some(node_id),
                kind: &source.kind,
                storage_uri: &source.storage_uri,
                sha256: source.sha256.as_deref(),
                mime: source.mime.as_deref(),
                width: source.width,
                height: source.height,
                duration_ms: source.duration_ms,
                selected: source.selected,
                meta_json: source.meta_json.as_deref(),
            })
            .await?)
    }

    pub(crate) async fn step_cache_key(
        &self,
        step: &ExecutionStep,
        inputs: &BTreeMap<String, ArtifactRef>,
    ) -> RunResult<StepCacheKey> {
        let provider = step
            .provider
            .clone()
            .unwrap_or_else(|| "builtin".to_owned());
        let mut input_hashes = BTreeMap::new();
        for (port, artifact_ref) in inputs {
            let artifact = self.store.artifact(&artifact_ref.artifact_id).await?;
            input_hashes.insert(port.clone(), artifact_fingerprint(&artifact)?);
        }
        let input_hash_json = serde_json::to_string(&input_hashes)?;
        // schemaVersion 4: the capability id renamed to canonical
        // `text_to_image` (GH145), so pre-rename cache entries must never be
        // reused. v3 added the resolved implementation (GH130 T4) — the model
        // identity lives on the step, so two runs resolving different
        // bindings can never share an artifact. Provider config still
        // participates for API-base changes (HF-021).
        let material = canonicalize_value(&json!({
            "schemaVersion": 4,
            "providerConfig": self.provider.config_fingerprint(&provider),
            "provider": &provider,
            "capability": step.capability.as_deref(),
            "nodeType": &step.node_type,
            "nodeId": &step.node_id,
            "resolved": &step.resolved,
            "params": &step.params,
            "inputs": input_hashes,
        }));
        Ok(StepCacheKey {
            provider,
            cache_key: format!("sha256:{}", sha256_hex(&serde_json::to_vec(&material)?)),
            input_hash_json,
        })
    }

    async fn cached_artifact_available(&self, artifact: &ArtifactRecord) -> RunResult<bool> {
        if !is_local_artifact_path(&artifact.storage_uri) {
            return Ok(true);
        }
        let path = self.artifact_root.join(&artifact.storage_uri);
        if tokio::fs::metadata(&path).await.is_err() {
            return Ok(false);
        }
        let Some(expected) = artifact.sha256.as_deref().and_then(normalize_sha256) else {
            return Ok(true);
        };
        Ok(file_sha256_hex(&path).await? == expected)
    }

    async fn emit_node_state_cached(
        &self,
        workspace_id: &str,
        run_id: &str,
        step: &ExecutionStep,
        cached: bool,
    ) -> RunResult<()> {
        self.emit(
            workspace_id,
            run_id,
            "node.state",
            json!({
                "node_id": step.node_id,
                "node_type": step.node_type,
                "provider": step.provider,
                "state": RunStepState::Succeeded.as_str(),
                "cached": cached
            }),
        )
        .await
    }
}

fn artifact_fingerprint(artifact: &ArtifactRecord) -> RunResult<String> {
    let material = canonicalize_value(&json!({
        "kind": &artifact.kind,
        "storageUri": &artifact.storage_uri,
        "sha256": artifact.sha256.as_deref(),
        "mime": artifact.mime.as_deref(),
        "width": artifact.width,
        "height": artifact.height,
        "durationMs": artifact.duration_ms,
        "meta": artifact.meta_json.as_deref(),
    }));
    Ok(format!(
        "sha256:{}",
        sha256_hex(&serde_json::to_vec(&material)?)
    ))
}

fn canonicalize_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_value).collect()),
        Value::Object(map) => {
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort();
            let mut sorted = serde_json::Map::new();
            for key in keys {
                sorted.insert(key.clone(), canonicalize_value(&map[key]));
            }
            Value::Object(sorted)
        }
        other => other.clone(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex_lower(&digest)
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

pub(crate) fn is_local_artifact_path(storage_uri: &str) -> bool {
    storage_uri.starts_with("artifacts/")
}

fn normalize_sha256(value: &str) -> Option<String> {
    let normalized = value.trim().strip_prefix("sha256:").unwrap_or(value.trim());
    (normalized.len() == 64 && normalized.chars().all(|ch| ch.is_ascii_hexdigit()))
        .then(|| normalized.to_ascii_lowercase())
}

pub(crate) async fn file_sha256_hex(path: &Path) -> RunResult<String> {
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|err| RunError::ArtifactPersistence(err.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|err| RunError::ArtifactPersistence(err.to_string()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_lower(&hasher.finalize()))
}
