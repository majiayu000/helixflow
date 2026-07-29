use std::collections::BTreeMap;
use std::time::Duration;

use helixflow_gateway::{ArtifactPayload, ArtifactRef, Provider, ProviderResult};
use helixflow_graph::ExecutionStep;
use helixflow_store::{
    ArtifactRecord, NewArtifact, NewArtifactPublishJournal, PROVIDER_TASK_RESULT_READY,
    ProviderTaskFailureFinalization, ProviderTaskRecord, RunStepRecord,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::artifact_path::validate_relative_path;
use super::artifacts::{persist_provider_artifact_at, recovery_artifact_relative_path};
use super::cache::{CachedArtifactLink, file_sha256_hex, is_local_artifact_path};
use super::executor::artifact_kind_label;
use super::provider_recovery::ProviderStepExecution;
use super::{RunError, RunResult, RunService, StepOutput};

const MATERIALIZATION_RETRY_SECONDS: i64 = 1;
const ARTIFACT_JOURNAL_EXPIRY_SECONDS: i64 = 3600;
const ARTIFACT_GC_LEASE_SECONDS: i64 = 60;

impl<P> RunService<P>
where
    P: Provider + Clone + Send + Sync + 'static,
{
    pub(crate) async fn reconcile_artifact_publish_journals(&self) -> RunResult<()> {
        for journal in self.store.pending_artifact_publish_journals().await? {
            if journal.state != "published" || journal.artifact_id.is_none() {
                continue;
            }
            let step = self.store.run_step(&journal.run_step_id).await?;
            if step.state == "succeeded" {
                self.store
                    .mark_artifact_journal_committed(&journal.operation_key, &journal.owner_id)
                    .await?;
            }
        }
        let gc_owner = format!("artifact-gc:{}", Uuid::now_v7());
        for journal in self
            .store
            .claim_expired_artifact_journals_for_gc(&gc_owner, ARTIFACT_GC_LEASE_SECONDS)
            .await?
        {
            let relative = journal
                .published_path
                .as_deref()
                .unwrap_or(&journal.staged_path);
            if is_local_artifact_path(relative) {
                let relative_path = std::path::Path::new(relative);
                validate_relative_path(relative_path)?;
                match tokio::fs::remove_file(self.artifact_root.join(relative_path)).await {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => {
                        return Err(RunError::ArtifactPersistence(
                            "expired artifact journal cleanup failed".to_owned(),
                        ));
                    }
                }
            }
            if !self
                .store
                .complete_artifact_journal_gc(&journal.operation_key, &gc_owner)
                .await?
            {
                return Err(RunError::ArtifactPersistence(
                    "expired artifact journal cleanup lost its lease".to_owned(),
                ));
            }
        }
        Ok(())
    }

    pub(crate) async fn materialize_provider_task(
        &self,
        workspace_id: &str,
        step: &ExecutionStep,
        record: &RunStepRecord,
        initial_task: &ProviderTaskRecord,
    ) -> RunResult<Option<ProviderStepExecution>> {
        let mut task = initial_task.clone();
        loop {
            let timing = self.store.provider_task_timing(&task.id).await?;
            if timing.materialization_expired {
                self.store
                    .finalize_provider_task_failure(ProviderTaskFailureFinalization {
                        provider_task_id: &task.id,
                        expected_task_state: PROVIDER_TASK_RESULT_READY,
                        terminal_task_state: "completed",
                        error_code: "ARTIFACT_MATERIALIZATION_DEADLINE",
                        desired_run_status: "failed",
                        error_json: Some(r#"{"error":"artifact materialization failed"}"#),
                        required_expired_foreign_owner: None,
                    })
                    .await?;
                return Err(RunError::ArtifactPersistence(
                    "provider artifact materialization deadline expired".to_owned(),
                ));
            }
            if !timing.materialization_due {
                tokio::time::sleep(Duration::from_secs(1)).await;
                task = self.store.provider_task(&task.id).await?;
                continue;
            }
            match self
                .materialize_provider_task_once(workspace_id, step, record, &task)
                .await
            {
                Ok(execution) => return Ok(Some(execution)),
                Err(err) if materialization_error_is_permanent(&err) => {
                    self.store
                        .finalize_provider_task_failure(ProviderTaskFailureFinalization {
                            provider_task_id: &task.id,
                            expected_task_state: PROVIDER_TASK_RESULT_READY,
                            terminal_task_state: "completed",
                            error_code: "ARTIFACT_MATERIALIZATION_INVALID",
                            desired_run_status: "failed",
                            error_json: Some(r#"{"error":"artifact materialization failed"}"#),
                            required_expired_foreign_owner: None,
                        })
                        .await?;
                    return Err(err);
                }
                Err(_) => {
                    let Some(updated) = self
                        .store
                        .record_materialization_failure(
                            &task.id,
                            MATERIALIZATION_RETRY_SECONDS,
                            "ARTIFACT_MATERIALIZATION_RETRY",
                        )
                        .await?
                    else {
                        return Err(RunError::ArtifactPersistence(
                            "provider artifact materialization deadline expired".to_owned(),
                        ));
                    };
                    task = updated;
                }
            }
        }
    }

    async fn materialize_provider_task_once(
        &self,
        workspace_id: &str,
        step: &ExecutionStep,
        record: &RunStepRecord,
        task: &ProviderTaskRecord,
    ) -> RunResult<ProviderStepExecution> {
        let relative = task.result_spool_path.as_deref().ok_or_else(|| {
            RunError::ArtifactPersistence("result_ready task is missing spool path".to_owned())
        })?;
        let bytes = tokio::fs::read(self.artifact_root.join(relative))
            .await
            .map_err(|_| {
                RunError::ArtifactPersistence("recovery spool cannot be read".to_owned())
            })?;
        let fingerprint = format!("sha256:{:x}", Sha256::digest(&bytes));
        if task.result_fingerprint.as_deref() != Some(fingerprint.as_str()) {
            return Err(RunError::ArtifactPersistence(
                "recovery spool fingerprint mismatch".to_owned(),
            ));
        }
        let result: ProviderResult = serde_json::from_slice(&bytes).map_err(|_| {
            RunError::ArtifactPersistence("recovery spool payload is invalid".to_owned())
        })?;
        let existing = self
            .store
            .run_step_outputs(&task.run_id)
            .await?
            .into_iter()
            .filter(|item| item.run_step_id == record.id)
            .map(|item| (item.port, item.artifact_id))
            .collect::<BTreeMap<_, _>>();
        let mut outputs = Vec::new();
        let mut cache_links = Vec::new();
        let mut journal_keys = Vec::new();
        for (port, payload) in result.outputs {
            let artifact = if let Some(artifact_id) = existing.get(&port) {
                self.store.artifact(artifact_id).await?
            } else {
                let (artifact, operation_key) = self
                    .publish_provider_artifact(
                        workspace_id,
                        &step.node_id,
                        record,
                        task,
                        &port,
                        payload,
                    )
                    .await?;
                journal_keys.push(operation_key);
                artifact
            };
            cache_links.push(CachedArtifactLink {
                port: Some(port.clone()),
                artifact_id: artifact.id.clone(),
            });
            outputs.push(StepOutput {
                port,
                artifact: ArtifactRef {
                    artifact_id: artifact.id,
                    storage_uri: artifact.storage_uri,
                },
            });
        }
        let cost_json = serde_json::to_string(&result.cost)?;
        let mappings = outputs
            .iter()
            .map(|output| (output.port.clone(), output.artifact.artifact_id.clone()))
            .collect::<Vec<_>>();
        if !self
            .store
            .finalize_run_step_success(&record.id, Some(&task.id), Some(&cost_json), &mappings)
            .await?
        {
            return Err(RunError::ArtifactPersistence(
                "provider step finalizer lost its durable state".to_owned(),
            ));
        }
        let owner_id = artifact_journal_owner(&task.id);
        for operation_key in journal_keys {
            self.store
                .mark_artifact_journal_committed(&operation_key, &owner_id)
                .await?;
        }
        Ok(ProviderStepExecution {
            outputs,
            cache_links,
            cost_json,
        })
    }

    async fn publish_provider_artifact(
        &self,
        workspace_id: &str,
        node_id: &str,
        record: &RunStepRecord,
        task: &ProviderTaskRecord,
        port: &str,
        payload: ArtifactPayload,
    ) -> RunResult<(ArtifactRecord, String)> {
        let payload_bytes = serde_json::to_vec(&payload)?;
        let content_sha256 = format!("sha256:{:x}", Sha256::digest(&payload_bytes));
        let planned_path = recovery_artifact_relative_path(&content_sha256, &payload)?;
        let operation_key = format!("artifact:{}:{port}", task.id);
        let owner_id = artifact_journal_owner(&task.id);
        let staged_path = planned_path.to_string_lossy();
        let journal = self
            .store
            .create_or_read_artifact_publish_journal(NewArtifactPublishJournal {
                operation_key: &operation_key,
                run_id: &task.run_id,
                run_step_id: &record.id,
                staged_path: &staged_path,
                content_sha256: &content_sha256,
                owner_id: &owner_id,
                expires_after_seconds: ARTIFACT_JOURNAL_EXPIRY_SECONDS,
            })
            .await?;
        if let Some(artifact_id) = journal.artifact_id {
            return Ok((self.store.artifact(&artifact_id).await?, operation_key));
        }
        let storage_uri =
            persist_provider_artifact_at(&self.artifact_root, &payload, &planned_path).await?;
        let sha256 = if is_local_artifact_path(&storage_uri) {
            Some(format!(
                "sha256:{}",
                file_sha256_hex(&self.artifact_root.join(&storage_uri)).await?
            ))
        } else {
            None
        };
        let meta_json = serde_json::to_string(&payload.meta)?;
        let artifact = self
            .store
            .publish_artifact_from_journal(
                &operation_key,
                &owner_id,
                &storage_uri,
                NewArtifact {
                    workspace_id,
                    run_id: Some(&task.run_id),
                    run_step_id: Some(&record.id),
                    node_id: Some(node_id),
                    kind: artifact_kind_label(payload.kind),
                    storage_uri: &storage_uri,
                    sha256: sha256.as_deref(),
                    mime: Some(&payload.mime),
                    width: payload.width.map(i64::from),
                    height: payload.height.map(i64::from),
                    duration_ms: payload.duration_ms.map(i64::from),
                    selected: false,
                    meta_json: Some(&meta_json),
                },
            )
            .await?;
        Ok((artifact, operation_key))
    }
}

fn artifact_journal_owner(task_id: &str) -> String {
    format!("materializer:{task_id}")
}

fn materialization_error_is_permanent(err: &RunError) -> bool {
    let RunError::ArtifactPersistence(message) = err else {
        return false;
    };
    [
        "missing spool path",
        "cannot be read",
        "fingerprint mismatch",
        "spool payload is invalid",
        "invalid image",
        "invalid video",
        "invalid declared media",
        "kind and MIME",
        "MIME is invalid",
        "no configured validator",
    ]
    .iter()
    .any(|marker| message.contains(marker))
}
