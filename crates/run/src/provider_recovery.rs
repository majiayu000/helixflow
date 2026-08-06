use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use helixflow_gateway::{
    DurableProviderTask, Provider, ProviderDispatch, ProviderDispatchFailureKind, ProviderRequest,
    ProviderResult, ProviderResume,
};
use helixflow_graph::ExecutionStep;
use helixflow_store::{
    NewProviderTask, PROVIDER_TASK_ACTIVE, PROVIDER_TASK_DISPATCHING, PROVIDER_TASK_RESULT_READY,
    ProviderTaskFailureFinalization, ProviderTaskHandleUpdate, ProviderTaskRecord,
    ProviderTaskResult, RunStepRecord,
};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use helixflow_graph::ExecutionPlan;

use super::cache::CachedArtifactLink;
use super::{
    RunError, RunInterrupt, RunResult, RunService, RunStatus, StepOutput, agent_fix_policy,
};

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
    pub fn validate_restart_config(&self) -> RunResult<()> {
        restart_requeue_enabled().map(|_| ())
    }

    pub async fn recover_after_restart(&self) -> RunResult<()> {
        let requeue = restart_requeue_enabled()?;
        let fix_enabled = agent_fix_policy()
            .map(|policy| policy.enabled)
            .unwrap_or(false);
        self.reconcile_artifact_publish_journals().await?;
        for work in self.store.pending_run_terminalizations().await? {
            let run = self.store.run(&work.run_id).await?;
            // Finish an already-requested terminal transition before scanning
            // active runs. Spawning this work allowed the same run to acquire
            // a recovery lease and resume provider execution while its remote
            // cancellation was still in flight.
            self.request_and_settle_terminal(
                &run.workspace_id,
                &run.id,
                &work.desired_status,
                work.error_json.as_deref(),
            )
            .await?;
        }
        let mut workspace_keepers = BTreeMap::<String, (String, Option<String>)>::new();
        for run in self.store.restart_active_runs().await? {
            if self
                .store
                .run_fix_attempt_for_child(&run.id)
                .await?
                .is_some()
                && self.is_quiescent_fix_child(&run.id).await?
                && (run.status == "estimating" || !fix_enabled)
            {
                continue;
            }
            if run.status == "running" {
                if let Some((_, keeper_group)) = workspace_keepers.get(&run.workspace_id)
                    && (run.group_id.is_none() || run.group_id != *keeper_group)
                {
                    self.request_and_settle_terminal(
                        &run.workspace_id,
                        &run.id,
                        RunStatus::Interrupted.as_str(),
                        None,
                    )
                    .await?;
                    continue;
                }
                workspace_keepers
                    .entry(run.workspace_id.clone())
                    .or_insert_with(|| (run.id.clone(), run.group_id.clone()));
            }
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
                    if !self.claim_restart_queued_run(&run).await? {
                        self.request_and_settle_terminal(
                            &run.workspace_id,
                            &run.id,
                            RunStatus::Interrupted.as_str(),
                            None,
                        )
                        .await?;
                        continue;
                    }
                    let running = self.store.run(&run.id).await?;
                    self.classify_running_restart(running).await?;
                }
                "running" => self.classify_running_restart(run).await?,
                _ => {}
            }
        }
        for continuation in self.store.pending_failure_continuations().await? {
            if continuation.state == "retry_created" {
                self.resume_retry_created(&continuation).await?;
                continue;
            }
            let runner = self.clone();
            tokio::spawn(async move {
                match runner.store.run(&continuation.run_id).await {
                    Ok(run) => {
                        if let Err(err) = runner.continue_self_heal_from_failed(&run).await {
                            eprintln!("failure continuation could not resume: {err}");
                        }
                    }
                    Err(err) => {
                        eprintln!("failure continuation parent could not be loaded: {err}");
                    }
                }
            });
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
        loop {
            let tasks = self.store.provider_tasks_for_run(&run.id).await?;
            let mut dispatch_owner_live = false;
            for task in tasks
                .iter()
                .filter(|task| task.state == PROVIDER_TASK_DISPATCHING)
            {
                dispatch_owner_live |= self
                    .store
                    .provider_task_timing(&task.id)
                    .await?
                    .dispatch_owner_live;
            }
            if !dispatch_owner_live {
                break;
            }
            if !self
                .store
                .renew_run_recovery_lease(&run.id, &owner_id, 60)
                .await?
            {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
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
            self.store
                .release_run_recovery_lease(&run.id, &owner_id)
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
        let all_steps_queued = !steps.is_empty() && steps.iter().all(|step| step.state == "queued");
        let incomplete_queued_intent =
            all_steps_queued && !self.restart_intent_is_complete(&run).await?;
        if run.plan_json.is_none() || missing_handle || unsafe_builtin || incomplete_queued_intent {
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
            self.store
                .release_run_recovery_lease(&run.id, &owner_id)
                .await?;
            return Ok(());
        }
        self.spawn_recovered_run(run, owner_id);
        Ok(())
    }

    fn spawn_recovered_run(&self, run: helixflow_store::RunRecord, owner_id: String) {
        let runner = self.clone();
        tokio::spawn(async move {
            if let Err(err) = runner.recover_running_run(run, owner_id).await {
                eprintln!("run recovery failed: {err}");
            }
        });
    }

    async fn recover_running_run(
        &self,
        run: helixflow_store::RunRecord,
        owner_id: String,
    ) -> RunResult<()> {
        let plan_json = run.plan_json.as_deref().ok_or_else(|| {
            RunError::InvalidConfiguration("running run is missing execution plan".to_owned())
        })?;
        let plan: ExecutionPlan = serde_json::from_str(plan_json)?;
        let interrupt = RunInterrupt::default();
        self.interrupts
            .lock()
            .await
            .insert(run.id.clone(), interrupt.clone());
        let heartbeat_runner = self.clone();
        let heartbeat_run_id = run.id.clone();
        let heartbeat_owner_id = owner_id.clone();
        let heartbeat_interrupt = interrupt.clone();
        let (stop_heartbeat, mut heartbeat_stopped) = tokio::sync::watch::channel(false);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(20)) => {
                        match heartbeat_runner
                            .store
                            .renew_run_recovery_lease(
                                &heartbeat_run_id,
                                &heartbeat_owner_id,
                                60,
                            )
                            .await
                        {
                            Ok(true) => {}
                            Ok(false) | Err(_) => {
                                heartbeat_interrupt.request();
                                return;
                            }
                        }
                    }
                    changed = heartbeat_stopped.changed() => {
                        if changed.is_err() || *heartbeat_stopped.borrow() {
                            return;
                        }
                    }
                }
            }
        });
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
        if stop_heartbeat.send(true).is_err() {
            eprintln!("run recovery heartbeat had already stopped");
        }
        self.store
            .release_run_recovery_lease(&run.id, &owner_id)
            .await?;
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
        let mut task = self
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
        loop {
            while task.state == PROVIDER_TASK_DISPATCHING
                && task.dispatch_owner_id.as_deref() != Some(owner_id.as_str())
                && self
                    .store
                    .provider_task_timing(&task.id)
                    .await?
                    .dispatch_owner_live
            {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {
                        task = self.store.provider_task(&task.id).await?;
                    }
                    _ = interrupt.cancelled() => return Ok(None),
                }
            }
            if task.state != PROVIDER_TASK_DISPATCHING
                || task.dispatch_owner_id.as_deref() == Some(owner_id.as_str())
            {
                break;
            }
            let abandoned = self
                .store
                .finalize_provider_task_failure(ProviderTaskFailureFinalization {
                    provider_task_id: &task.id,
                    expected_task_state: PROVIDER_TASK_DISPATCHING,
                    terminal_task_state: "abandoned",
                    error_code: "DISPATCH_OWNER_LOST",
                    desired_run_status: "interrupted",
                    error_json: None,
                    required_expired_foreign_owner: Some(&owner_id),
                })
                .await?;
            if abandoned {
                self.emit_billing_risk(workspace_id, run_id, &provider, "dispatch_unknown")
                    .await?;
                self.request_and_settle_terminal(workspace_id, run_id, "interrupted", None)
                    .await?;
                return Ok(None);
            }
            task = self.store.provider_task(&task.id).await?;
        }

        let result = match task.state.as_str() {
            PROVIDER_TASK_DISPATCHING => {
                match self
                    .dispatch_with_durable_owner(&task.id, &owner_id, request.clone(), interrupt)
                    .await?
                {
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
                        match self
                            .resume_provider_task(workspace_id, &task, &request, interrupt)
                            .await
                        {
                            Err(RunError::Interrupted(_)) => return Ok(None),
                            result => result?,
                        }
                    }
                    Err(failure) => {
                        if interrupt.is_requested()
                            && failure.kind == ProviderDispatchFailureKind::NotSubmitted
                        {
                            self.store
                                .complete_provider_task(
                                    &task.id,
                                    PROVIDER_TASK_DISPATCHING,
                                    Some("LOCAL_DISPATCH_INTERRUPTED"),
                                )
                                .await?;
                            return Ok(None);
                        }
                        let code = match failure.kind {
                            ProviderDispatchFailureKind::NotSubmitted => "PROVIDER_NOT_SUBMITTED",
                            ProviderDispatchFailureKind::Rejected => "PROVIDER_REJECTED",
                            ProviderDispatchFailureKind::OutcomeUnknown => {
                                self.store
                                    .finalize_provider_task_failure(
                                        ProviderTaskFailureFinalization {
                                            provider_task_id: &task.id,
                                            expected_task_state: PROVIDER_TASK_DISPATCHING,
                                            terminal_task_state: "abandoned",
                                            error_code: "DISPATCH_OUTCOME_UNKNOWN",
                                            desired_run_status: "interrupted",
                                            error_json: None,
                                            required_expired_foreign_owner: None,
                                        },
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
                            .finalize_provider_task_failure(ProviderTaskFailureFinalization {
                                provider_task_id: &task.id,
                                expected_task_state: PROVIDER_TASK_DISPATCHING,
                                terminal_task_state: "completed",
                                error_code: code,
                                desired_run_status: "failed",
                                error_json: Some(r#"{"error":"provider execution failed"}"#),
                                required_expired_foreign_owner: None,
                            })
                            .await?;
                        return Err(RunError::Provider(failure.error));
                    }
                }
            }
            PROVIDER_TASK_ACTIVE => {
                match self
                    .resume_provider_task(workspace_id, &task, &request, interrupt)
                    .await
                {
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
            .spool_provider_result(workspace_id, &task, &result, "succeeded")
            .await?;
        self.materialize_provider_task(workspace_id, step, record, &task)
            .await
    }

    async fn resume_provider_task(
        &self,
        workspace_id: &str,
        task: &ProviderTaskRecord,
        request: &ProviderRequest,
        interrupt: &RunInterrupt,
    ) -> RunResult<ProviderResult> {
        let durable = durable_task(task)?;
        loop {
            if interrupt.is_requested() {
                return Err(RunError::Interrupted(task.run_id.clone()));
            }
            if self
                .store
                .provider_task_timing(&task.id)
                .await?
                .recovery_expired
            {
                if self.provider.cancel_durable(&durable).await.is_ok() {
                    self.store.cancel_provider_task(&task.id, None).await?;
                    self.emit(
                        workspace_id,
                        &task.run_id,
                        "run.recovery_cancelled",
                        json!({ "provider": task.provider }),
                    )
                    .await?;
                } else {
                    self.store
                        .abandon_provider_task(
                            &task.id,
                            PROVIDER_TASK_ACTIVE,
                            "RECOVERY_DEADLINE_EXPIRED",
                        )
                        .await?;
                    self.emit_billing_risk(
                        workspace_id,
                        &task.run_id,
                        &task.provider,
                        "recovery_deadline",
                    )
                    .await?;
                }
                return Err(RunError::Interrupted(task.run_id.clone()));
            }
            match self.provider.resume(&durable, request).await? {
                ProviderResume::Completed(result) => return Ok(result),
                ProviderResume::Failed { reason_code, cost } => {
                    let result = ProviderResult {
                        outputs: BTreeMap::new(),
                        cost,
                    };
                    let ready = self
                        .spool_provider_result(workspace_id, task, &result, "failed")
                        .await?;
                    self.store
                        .finalize_provider_task_failure(ProviderTaskFailureFinalization {
                            provider_task_id: &ready.id,
                            expected_task_state: PROVIDER_TASK_RESULT_READY,
                            terminal_task_state: "completed",
                            error_code: &reason_code,
                            desired_run_status: "failed",
                            error_json: Some(r#"{"error":"provider execution failed"}"#),
                            required_expired_foreign_owner: None,
                        })
                        .await?;
                    return Err(RunError::Provider(
                        helixflow_gateway::ProviderError::RequestRejected(reason_code),
                    ));
                }
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
        terminal_outcome: &str,
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
                terminal_outcome,
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

    async fn emit_billing_risk(
        &self,
        workspace_id: &str,
        run_id: &str,
        provider: &str,
        reason_code: &str,
    ) -> RunResult<()> {
        self.persist_billing_risk(workspace_id, run_id, provider, reason_code)
            .await
    }
}

pub(crate) fn durable_task(task: &ProviderTaskRecord) -> RunResult<DurableProviderTask> {
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
