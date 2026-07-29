use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use helixflow_gateway::{
    ArtifactRef, CostEstimate, DurableProviderTask, Provider, ProviderDispatch,
    ProviderDispatchFailureKind, ProviderRequest, ProviderResult, ProviderResume,
};
use helixflow_graph::ExecutionStep;
use helixflow_store::{
    NewCostLedger, NewProviderTask, PROVIDER_TASK_ACTIVE, PROVIDER_TASK_DISPATCHING,
    PROVIDER_TASK_RESULT_READY, ProviderTaskHandleUpdate, ProviderTaskRecord, ProviderTaskResult,
    RunStepRecord,
};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::cache::CachedArtifactLink;
use helixflow_graph::ExecutionPlan;

use super::{RunError, RunInterrupt, RunOutcome, RunResult, RunService, RunStatus, StepOutput};

const DISPATCH_LEASE_SECONDS: i64 = 60;
const DISPATCH_DEADLINE_SECONDS: i64 = 300;
const RECOVERY_DEADLINE_SECONDS: i64 = 3600;
const MATERIALIZATION_DEADLINE_SECONDS: i64 = 3600;

pub(crate) struct ProviderStepExecution {
    pub(crate) outputs: Vec<StepOutput>,
    pub(crate) cache_links: Vec<CachedArtifactLink>,
    pub(crate) cost_json: String,
}

impl<P> RunService<P>
where
    P: Provider + Clone + Send + Sync + 'static,
{
    pub async fn recover_after_restart(&self) -> RunResult<()> {
        let requeue = restart_requeue_enabled()?;
        for run in self.store.restart_active_runs().await? {
            match run.status.as_str() {
                "estimating" => {
                    self.request_and_settle_terminal(
                        &run.workspace_id,
                        &run.id,
                        RunStatus::Interrupted.as_str(),
                        None,
                    )
                    .await?;
                }
                "queued" if !requeue => {
                    self.request_and_settle_terminal(
                        &run.workspace_id,
                        &run.id,
                        RunStatus::Interrupted.as_str(),
                        None,
                    )
                    .await?;
                }
                "queued" => {
                    if self.store.run_execution_intent(&run.id).await?.is_none() {
                        self.request_and_settle_terminal(
                            &run.workspace_id,
                            &run.id,
                            RunStatus::Interrupted.as_str(),
                            None,
                        )
                        .await?;
                        continue;
                    }
                    self.spawn_recovered_run(run);
                }
                "running" => self.classify_running_restart(run).await?,
                _ => {}
            }
        }
        Ok(())
    }

    pub(crate) async fn ensure_execution_intent(
        &self,
        run_id: &str,
        plan: &ExecutionPlan,
        estimate_json: Option<&str>,
    ) -> RunResult<()> {
        let plan_bytes = serde_json::to_vec(plan)?;
        let plan_fingerprint = format!("sha256:{:x}", Sha256::digest(&plan_bytes));
        let estimate_fingerprint = format!(
            "sha256:{:x}",
            Sha256::digest(estimate_json.unwrap_or("").as_bytes())
        );
        self.store
            .create_or_read_execution_intent(
                run_id,
                &plan_fingerprint,
                &estimate_fingerprint,
                "approved",
            )
            .await?;
        Ok(())
    }

    async fn classify_running_restart(&self, run: helixflow_store::RunRecord) -> RunResult<()> {
        let owner_id = format!("recovery-{}", Uuid::now_v7());
        if self
            .store
            .claim_run_recovery_lease(&run.id, &owner_id, 60)
            .await?
            .is_none()
        {
            return Ok(());
        }
        let tasks = self.store.provider_tasks_for_run(&run.id).await?;
        if tasks
            .iter()
            .any(|task| task.state == PROVIDER_TASK_DISPATCHING)
        {
            for task in tasks
                .iter()
                .filter(|task| task.state == PROVIDER_TASK_DISPATCHING)
            {
                self.store
                    .abandon_provider_task(
                        &task.id,
                        PROVIDER_TASK_DISPATCHING,
                        "DISPATCH_OWNER_LOST",
                    )
                    .await?;
                self.emit_billing_risk(
                    &run.workspace_id,
                    &run.id,
                    &task.provider,
                    "dispatch_unknown",
                )
                .await?;
            }
            self.request_and_settle_terminal(
                &run.workspace_id,
                &run.id,
                RunStatus::Interrupted.as_str(),
                None,
            )
            .await?;
            return Ok(());
        }
        let steps = self.store.run_steps(&run.id).await?;
        let missing_handle = steps.iter().any(|step| {
            step.state == "running"
                && step.provider.is_some()
                && !tasks.iter().any(|task| task.run_step_id == step.id)
        });
        let unsafe_builtin = steps.iter().any(|step| {
            step.state == "running"
                && step.provider.is_none()
                && !matches!(
                    step.node_type.as_str(),
                    "input.text" | "input.image" | "output.save"
                )
        });
        if run.plan_json.is_none() || missing_handle || unsafe_builtin {
            if missing_handle {
                self.emit_billing_risk(&run.workspace_id, &run.id, "unknown", "missing_handle")
                    .await?;
            }
            self.request_and_settle_terminal(
                &run.workspace_id,
                &run.id,
                RunStatus::Interrupted.as_str(),
                None,
            )
            .await?;
            return Ok(());
        }
        self.spawn_recovered_run(run);
        Ok(())
    }

    fn spawn_recovered_run(&self, run: helixflow_store::RunRecord) {
        let runner = self.clone();
        tokio::spawn(async move {
            if let Err(err) = runner.recover_running_run(run).await {
                eprintln!("run recovery failed: {err}");
            }
        });
    }

    async fn recover_running_run(&self, run: helixflow_store::RunRecord) -> RunResult<()> {
        let plan_json = run.plan_json.as_deref().ok_or_else(|| {
            RunError::InvalidConfiguration("running run is missing execution plan".to_owned())
        })?;
        let plan: ExecutionPlan = serde_json::from_str(plan_json)?;
        let interrupt = RunInterrupt::default();
        self.interrupts
            .lock()
            .await
            .insert(run.id.clone(), interrupt.clone());
        self.emit(
            &run.workspace_id,
            &run.id,
            "run.recovery_started",
            json!({ "reason_code": "server_restart" }),
        )
        .await?;
        let result = self
            .execute_created_run(&run, &run.workspace_id, &plan, interrupt, run.force_rerun)
            .await;
        self.interrupts.lock().await.remove(&run.id);
        if result
            .as_ref()
            .is_ok_and(|outcome| outcome.run.status == "succeeded")
        {
            self.emit(
                &run.workspace_id,
                &run.id,
                "run.recovery_succeeded",
                json!({}),
            )
            .await?;
        }
        result.map(|_| ())
    }

    pub(crate) async fn record_actual_costs(&self, outcome: &RunOutcome) -> RunResult<()> {
        for step in &outcome.steps {
            let (Some(provider), Some(cost_json)) = (&step.provider, &step.cost_actual_json) else {
                continue;
            };
            if self.store.provider_task_for_step(&step.id).await?.is_some() {
                continue;
            }
            let cost: CostEstimate = serde_json::from_str(cost_json)?;
            self.store
                .create_cost_ledger(NewCostLedger {
                    workspace_id: &outcome.run.workspace_id,
                    run_id: Some(&outcome.run.id),
                    run_step_id: Some(&step.id),
                    provider,
                    amount: cost.amount,
                    currency: &cost.currency,
                    estimated: cost.estimated,
                })
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn execute_provider_step(
        &self,
        workspace_id: &str,
        run_id: &str,
        step: &ExecutionStep,
        record: &RunStepRecord,
        request: ProviderRequest,
        interrupt: &RunInterrupt,
    ) -> RunResult<Option<ProviderStepExecution>> {
        let provider = request.provider.clone();
        let origin = self.provider.dispatch_origin(&provider)?;
        let scope = self.provider.recovery_scope_fingerprint(&provider);
        let owner_id = format!("dispatch-{}", Uuid::now_v7());
        let operation_key = format!("dispatch:{run_id}:{}:{provider}", record.id);
        let task = self
            .store
            .insert_or_read_provider_task(NewProviderTask {
                run_id,
                run_step_id: &record.id,
                provider: &provider,
                dispatch_origin: &origin,
                recovery_scope_fingerprint: &scope,
                operation_key: &operation_key,
                dispatch_owner_id: &owner_id,
                dispatch_lease_seconds: DISPATCH_LEASE_SECONDS,
                dispatch_deadline_seconds: DISPATCH_DEADLINE_SECONDS,
            })
            .await?;

        let result = match task.state.as_str() {
            PROVIDER_TASK_DISPATCHING => match tokio::select! {
                result = self.provider.dispatch(request.clone()) => Some(result),
                _ = interrupt.cancelled() => None,
            } {
                None => {
                    self.store
                        .abandon_provider_task(
                            &task.id,
                            PROVIDER_TASK_DISPATCHING,
                            "DISPATCH_OUTCOME_UNKNOWN",
                        )
                        .await?;
                    self.emit_billing_risk(workspace_id, run_id, &provider, "dispatch_unknown")
                        .await?;
                    return Ok(None);
                }
                Some(result) => match result {
                    Ok(ProviderDispatch::Completed(result)) => result,
                    Ok(ProviderDispatch::Accepted(handle)) => {
                        let task = self
                            .store
                            .activate_provider_task(ProviderTaskHandleUpdate {
                                task_id: &task.id,
                                dispatch_owner_id: &owner_id,
                                provider_task_id: &handle.provider_task_id,
                                status_url: handle.status_url.as_deref(),
                                result_url: handle.result_url.as_deref(),
                                recovery_deadline_seconds: RECOVERY_DEADLINE_SECONDS,
                            })
                            .await?
                            .ok_or_else(|| {
                                RunError::ArtifactPersistence(
                                    "provider dispatch handle lost its durable owner".to_owned(),
                                )
                            })?;
                        if interrupt.is_requested() {
                            self.request_and_settle_terminal(
                                workspace_id,
                                run_id,
                                "interrupted",
                                None,
                            )
                            .await?;
                            return Ok(None);
                        }
                        match self.resume_provider_task(&task, &request, interrupt).await {
                            Err(RunError::Interrupted(_)) => return Ok(None),
                            result => result?,
                        }
                    }
                    Err(failure) => {
                        let code = match failure.kind {
                            ProviderDispatchFailureKind::NotSubmitted => "PROVIDER_NOT_SUBMITTED",
                            ProviderDispatchFailureKind::Rejected => "PROVIDER_REJECTED",
                            ProviderDispatchFailureKind::OutcomeUnknown => {
                                self.store
                                    .abandon_provider_task(
                                        &task.id,
                                        PROVIDER_TASK_DISPATCHING,
                                        "DISPATCH_OUTCOME_UNKNOWN",
                                    )
                                    .await?;
                                self.emit_billing_risk(
                                    workspace_id,
                                    run_id,
                                    &provider,
                                    "dispatch_unknown",
                                )
                                .await?;
                                self.request_and_settle_terminal(
                                    workspace_id,
                                    run_id,
                                    "interrupted",
                                    None,
                                )
                                .await?;
                                return Ok(None);
                            }
                        };
                        self.store
                            .complete_provider_task(&task.id, PROVIDER_TASK_DISPATCHING, Some(code))
                            .await?;
                        return Err(RunError::Provider(failure.error));
                    }
                },
            },
            PROVIDER_TASK_ACTIVE => {
                match self.resume_provider_task(&task, &request, interrupt).await {
                    Err(RunError::Interrupted(_)) => return Ok(None),
                    result => result?,
                }
            }
            PROVIDER_TASK_RESULT_READY => {
                return self
                    .materialize_provider_task(workspace_id, step, record, &task)
                    .await;
            }
            other => {
                return Err(RunError::ArtifactPersistence(format!(
                    "provider task for step `{}` is unexpectedly `{other}`",
                    record.id
                )));
            }
        };

        let task = self
            .spool_provider_result(workspace_id, &task, &result)
            .await?;
        self.materialize_provider_task(workspace_id, step, record, &task)
            .await
    }

    async fn resume_provider_task(
        &self,
        task: &ProviderTaskRecord,
        request: &ProviderRequest,
        interrupt: &RunInterrupt,
    ) -> RunResult<ProviderResult> {
        let durable = durable_task(task)?;
        loop {
            if interrupt.is_requested() {
                return Err(RunError::Interrupted(task.run_id.clone()));
            }
            match self.provider.resume(&durable, request).await? {
                ProviderResume::Completed(result) => return Ok(result),
                ProviderResume::Pending { retry_after_ms } => {
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(retry_after_ms.max(25))) => {}
                        _ = interrupt.cancelled() => {
                            return Err(RunError::Interrupted(task.run_id.clone()));
                        }
                    }
                }
            }
        }
    }

    async fn spool_provider_result(
        &self,
        workspace_id: &str,
        task: &ProviderTaskRecord,
        result: &ProviderResult,
    ) -> RunResult<ProviderTaskRecord> {
        let mut safe_result = result.clone();
        for payload in safe_result.outputs.values_mut() {
            payload.meta = sanitize_metadata(&payload.meta);
        }
        let bytes = serde_json::to_vec(&safe_result)?;
        let fingerprint = format!("sha256:{:x}", Sha256::digest(&bytes));
        let relative = PathBuf::from("recovery_spool").join(format!("{}.json", task.id));
        let full_path = self.artifact_root.join(&relative);
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|err| {
                RunError::ArtifactPersistence(format!("create recovery spool: {err}"))
            })?;
        }
        let mut file = tokio::fs::File::create(&full_path).await.map_err(|err| {
            RunError::ArtifactPersistence(format!("create recovery spool: {err}"))
        })?;
        use tokio::io::AsyncWriteExt;
        file.write_all(&bytes)
            .await
            .map_err(|err| RunError::ArtifactPersistence(format!("write recovery spool: {err}")))?;
        file.sync_all()
            .await
            .map_err(|err| RunError::ArtifactPersistence(format!("sync recovery spool: {err}")))?;
        self.store
            .mark_provider_task_result_ready(ProviderTaskResult {
                task_id: &task.id,
                dispatch_owner_id: task.dispatch_owner_id.as_deref(),
                terminal_outcome: "succeeded",
                result_spool_path: &relative.to_string_lossy(),
                result_fingerprint: &fingerprint,
                materialization_deadline_seconds: MATERIALIZATION_DEADLINE_SECONDS,
                workspace_id,
                provider: &task.provider,
                amount: safe_result.cost.amount,
                currency: &safe_result.cost.currency,
                estimated: safe_result.cost.estimated,
            })
            .await?
            .ok_or_else(|| {
                RunError::ArtifactPersistence(
                    "provider result lost its durable task transition".to_owned(),
                )
            })
    }

    async fn materialize_provider_task(
        &self,
        workspace_id: &str,
        step: &ExecutionStep,
        record: &RunStepRecord,
        task: &ProviderTaskRecord,
    ) -> RunResult<Option<ProviderStepExecution>> {
        let relative = task.result_spool_path.as_deref().ok_or_else(|| {
            RunError::ArtifactPersistence("result_ready task is missing spool path".to_owned())
        })?;
        let bytes = tokio::fs::read(self.artifact_root.join(relative))
            .await
            .map_err(|err| RunError::ArtifactPersistence(format!("read recovery spool: {err}")))?;
        let fingerprint = format!("sha256:{:x}", Sha256::digest(&bytes));
        if task.result_fingerprint.as_deref() != Some(fingerprint.as_str()) {
            return Err(RunError::ArtifactPersistence(
                "recovery spool fingerprint mismatch".to_owned(),
            ));
        }
        let result: ProviderResult = serde_json::from_slice(&bytes)?;
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
        for (port, payload) in result.outputs {
            let artifact = if let Some(artifact_id) = existing.get(&port) {
                self.store.artifact(artifact_id).await?
            } else {
                let artifact = self
                    .persist_artifact(
                        workspace_id,
                        &task.run_id,
                        &record.id,
                        &step.node_id,
                        payload,
                    )
                    .await?;
                self.store
                    .link_run_step_output(&record.id, &port, &artifact.id)
                    .await?;
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
        self.store
            .complete_provider_task(&task.id, PROVIDER_TASK_RESULT_READY, None)
            .await?;
        Ok(Some(ProviderStepExecution {
            outputs,
            cache_links,
            cost_json: serde_json::to_string(&result.cost)?,
        }))
    }

    pub(crate) async fn request_and_settle_terminal(
        &self,
        workspace_id: &str,
        run_id: &str,
        desired_status: &str,
        error_json: Option<&str>,
    ) -> RunResult<()> {
        self.store
            .request_run_terminalization(run_id, desired_status, error_json)
            .await?;
        let owner_id = format!("settler-{}", Uuid::now_v7());
        if !self
            .store
            .claim_run_terminalization(run_id, &owner_id, 60)
            .await?
        {
            return Ok(());
        }
        for task in self.store.provider_tasks_for_run(run_id).await? {
            match task.state.as_str() {
                PROVIDER_TASK_ACTIVE => {
                    match self.provider.cancel_durable(&durable_task(&task)?).await {
                        Ok(()) => {
                            self.store.cancel_provider_task(&task.id, None).await?;
                            self.emit(
                                workspace_id,
                                run_id,
                                "run.recovery_cancelled",
                                json!({ "provider": task.provider }),
                            )
                            .await?;
                        }
                        Err(_) => {
                            self.store
                                .abandon_provider_task(
                                    &task.id,
                                    PROVIDER_TASK_ACTIVE,
                                    "REMOTE_CANCEL_UNCONFIRMED",
                                )
                                .await?;
                            self.emit_billing_risk(
                                workspace_id,
                                run_id,
                                &task.provider,
                                "cancel_unconfirmed",
                            )
                            .await?;
                        }
                    }
                }
                PROVIDER_TASK_DISPATCHING => {
                    self.store
                        .abandon_provider_task(
                            &task.id,
                            PROVIDER_TASK_DISPATCHING,
                            "DISPATCH_OWNER_STOPPED",
                        )
                        .await?;
                    self.emit_billing_risk(
                        workspace_id,
                        run_id,
                        &task.provider,
                        "dispatch_unknown",
                    )
                    .await?;
                }
                PROVIDER_TASK_RESULT_READY => {
                    self.store
                        .complete_provider_task(
                            &task.id,
                            PROVIDER_TASK_RESULT_READY,
                            Some("ARTIFACT_MATERIALIZATION_FAILED"),
                        )
                        .await?;
                }
                _ => {}
            }
        }
        if let Some(run) = self
            .store
            .complete_run_terminalization(run_id, &owner_id)
            .await?
        {
            self.emit(
                workspace_id,
                run_id,
                if run.status == "failed" {
                    "run.failed"
                } else {
                    "run.interrupted"
                },
                error_json
                    .and_then(|value| serde_json::from_str(value).ok())
                    .unwrap_or_else(|| json!({})),
            )
            .await?;
        }
        Ok(())
    }

    async fn emit_billing_risk(
        &self,
        workspace_id: &str,
        run_id: &str,
        provider: &str,
        reason_code: &str,
    ) -> RunResult<()> {
        self.emit(
            workspace_id,
            run_id,
            "run.recovery_abandoned",
            json!({
                "provider": provider,
                "reason_code": reason_code,
                "message": "远端任务终态无法确认，可能继续产生费用。"
            }),
        )
        .await
    }
}

fn durable_task(task: &ProviderTaskRecord) -> RunResult<DurableProviderTask> {
    Ok(DurableProviderTask {
        provider: task.provider.clone(),
        provider_task_id: task.provider_task_id.clone().ok_or_else(|| {
            RunError::ArtifactPersistence("active provider task is missing identity".to_owned())
        })?,
        dispatch_origin: task.dispatch_origin.clone(),
        recovery_scope_fingerprint: task.recovery_scope_fingerprint.clone(),
        status_url: task.status_url.clone(),
        result_url: task.result_url.clone(),
    })
}

fn sanitize_metadata(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, _)| !is_sensitive_metadata_key(key))
                .map(|(key, value)| (key.clone(), sanitize_metadata(value)))
                .collect::<Map<_, _>>(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(sanitize_metadata).collect()),
        other => other.clone(),
    }
}

fn is_sensitive_metadata_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "task_id",
        "request_id",
        "prediction_id",
        "authorization",
        "token",
        "cookie",
        "status_url",
        "result_url",
        "url",
    ]
    .iter()
    .any(|needle| key == *needle || key.ends_with(needle))
}

fn restart_requeue_enabled() -> RunResult<bool> {
    match std::env::var("HELIXFLOW_RUN_REQUEUE_ON_RESTART") {
        Err(std::env::VarError::NotPresent) => Ok(false),
        Err(std::env::VarError::NotUnicode(_)) => Err(RunError::InvalidConfiguration(
            "HELIXFLOW_RUN_REQUEUE_ON_RESTART must be valid UTF-8".to_owned(),
        )),
        Ok(value) => match value.trim() {
            "true" | "1" => Ok(true),
            "false" | "0" => Ok(false),
            _ => Err(RunError::InvalidConfiguration(
                "HELIXFLOW_RUN_REQUEUE_ON_RESTART must be true, false, 1, or 0".to_owned(),
            )),
        },
    }
}
