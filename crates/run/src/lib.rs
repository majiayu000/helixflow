use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use helixflow_gateway::{ArtifactKind, ArtifactRef, MockProvider, Provider, ProviderRequest};
use helixflow_graph::{ExecutionPlan, ExecutionStep, GraphService, WorkflowGraph};
use helixflow_registry::NodeRegistry;
use helixflow_store::{
    ArtifactRecord, NewArtifact, NewRun, NewRunStep, RunRecord, RunStepRecord, Store,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Notify, broadcast};

mod artifacts;
mod cost_gate;
mod error;

use artifacts::{default_artifact_root, persist_provider_artifact};
pub use cost_gate::{
    AgentRunRequest, CostSummary, PendingRun, PendingSweep, SweepOutcome, SweepPlan, SweepVariant,
};
pub use error::{RunError, RunResult};

pub fn module_name() -> &'static str {
    "run"
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Estimating,
    WaitingConfirmation,
    Running,
    Succeeded,
    Failed,
    Interrupted,
}

impl RunStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Estimating => "estimating",
            Self::WaitingConfirmation => "waiting_confirmation",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStepState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Skipped,
}

impl RunStepState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ManualRunRequest {
    pub workspace_id: String,
    pub version_id: String,
    pub group_id: Option<String>,
    pub label: String,
    pub provider: String,
    pub graph: WorkflowGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunEventEnvelope {
    pub workspace_id: String,
    pub run_id: String,
    pub seq: i64,
    pub server_time: String,
    pub ev: String,
    pub data: Value,
}

#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub run: RunRecord,
    pub steps: Vec<RunStepRecord>,
    pub artifacts: Vec<ArtifactRecord>,
}

#[derive(Debug, Clone)]
pub struct EventBus {
    sender: broadcast::Sender<RunEventEnvelope>,
}

impl EventBus {
    pub fn new(buffer: usize) -> Self {
        let (sender, _) = broadcast::channel(buffer);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RunEventEnvelope> {
        self.sender.subscribe()
    }

    pub fn publish(
        &self,
        event: RunEventEnvelope,
    ) -> Result<usize, broadcast::error::SendError<RunEventEnvelope>> {
        self.sender.send(event)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(128)
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunInterrupt {
    requested: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl RunInterrupt {
    pub fn request(&self) {
        let was_requested = self.requested.swap(true, Ordering::SeqCst);
        if !was_requested {
            self.notify.notify_waiters();
        }
    }

    fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    async fn cancelled(&self) {
        if self.is_requested() {
            return;
        }

        let notified = self.notify.notified();
        if self.is_requested() {
            return;
        }

        notified.await;
    }
}

#[derive(Clone)]
pub struct RunService<P = MockProvider> {
    store: Store,
    graph: GraphService,
    provider: P,
    artifact_root: PathBuf,
    events: EventBus,
    interrupts: Arc<Mutex<BTreeMap<String, RunInterrupt>>>,
}

impl RunService<MockProvider> {
    pub fn new(store: Store) -> Self {
        Self::with_provider(store, MockProvider::new())
    }
}

impl<P> RunService<P>
where
    P: Provider + Clone + Send + Sync + 'static,
{
    pub fn with_provider(store: Store, provider: P) -> Self {
        Self::with_provider_and_events(store, provider, EventBus::default())
    }

    pub fn with_provider_and_events(store: Store, provider: P, events: EventBus) -> Self {
        Self::with_provider_events_and_artifact_root(
            store,
            provider,
            events,
            default_artifact_root(),
        )
    }

    pub fn with_provider_events_and_artifact_root(
        store: Store,
        provider: P,
        events: EventBus,
        artifact_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            store,
            graph: GraphService::new(NodeRegistry::builtin()),
            provider,
            artifact_root: artifact_root.into(),
            events,
            interrupts: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn events(&self) -> EventBus {
        self.events.clone()
    }

    pub async fn interrupt_run(&self, run_id: &str) -> RunResult<()> {
        let interrupts = self.interrupts.lock().await;
        let interrupt = interrupts
            .get(run_id)
            .ok_or_else(|| RunError::RunNotActive(run_id.to_owned()))?;
        interrupt.request();
        Ok(())
    }

    pub async fn execute_manual_run(&self, request: ManualRunRequest) -> RunResult<RunOutcome> {
        let plan =
            self.graph
                .compile_plan(&request.graph, &request.version_id, &request.provider)?;
        if plan.steps.is_empty() {
            return Err(RunError::NoExecutableSteps);
        }
        let plan_json = serde_json::to_string(&plan)?;
        let run = self
            .store
            .create_run(NewRun {
                workspace_id: &request.workspace_id,
                version_id: &request.version_id,
                group_id: request.group_id.as_deref(),
                label: &request.label,
                trigger: "manual",
                plan_json: Some(&plan_json),
                estimate_json: None,
                status: RunStatus::Queued.as_str(),
            })
            .await?;
        let interrupt = RunInterrupt::default();
        self.interrupts
            .lock()
            .await
            .insert(run.id.clone(), interrupt.clone());

        let result = self
            .execute_created_run(&run, &request.workspace_id, &plan, interrupt)
            .await;
        self.interrupts.lock().await.remove(&run.id);
        result
    }

    pub(crate) async fn execute_created_run(
        &self,
        run: &RunRecord,
        workspace_id: &str,
        plan: &ExecutionPlan,
        interrupt: RunInterrupt,
    ) -> RunResult<RunOutcome> {
        let steps = self.ensure_run_steps(&run.id, plan).await?;

        self.store
            .update_run_status(&run.id, RunStatus::Running.as_str(), None)
            .await?;
        self.emit(
            workspace_id,
            &run.id,
            "run.started",
            json!({ "version_id": plan.version_id }),
        )
        .await?;

        let mut outputs = BTreeMap::new();
        for (index, (step, record)) in plan.steps.iter().zip(steps.iter()).enumerate() {
            if interrupt.is_requested() {
                self.skip_steps(workspace_id, &run.id, &steps[index..])
                    .await?;
                self.store
                    .update_run_status(&run.id, RunStatus::Interrupted.as_str(), None)
                    .await?;
                self.emit(
                    workspace_id,
                    &run.id,
                    "run.interrupted",
                    json!({ "at_node_id": step.node_id }),
                )
                .await?;
                return self.outcome(&run.id).await;
            }

            match self
                .execute_step(
                    workspace_id,
                    &run.id,
                    step,
                    record,
                    &mut outputs,
                    &interrupt,
                )
                .await
            {
                Ok(StepExecution::Succeeded) => {}
                Ok(StepExecution::Interrupted) => {
                    self.skip_steps(workspace_id, &run.id, &steps[index..])
                        .await?;
                    self.store
                        .update_run_status(&run.id, RunStatus::Interrupted.as_str(), None)
                        .await?;
                    self.emit(
                        workspace_id,
                        &run.id,
                        "run.interrupted",
                        json!({ "at_node_id": step.node_id }),
                    )
                    .await?;
                    return self.outcome(&run.id).await;
                }
                Err(err) => {
                    let error_json = serde_json::to_string(&json!({ "error": err.to_string() }))?;
                    self.store
                        .update_run_step_state(
                            &record.id,
                            RunStepState::Failed.as_str(),
                            Some(1.0),
                            None,
                            Some(&error_json),
                        )
                        .await?;
                    self.emit_node_state(
                        workspace_id,
                        &run.id,
                        step,
                        RunStepState::Failed,
                        Some(&err.to_string()),
                    )
                    .await?;
                    self.skip_steps(workspace_id, &run.id, &steps[(index + 1)..])
                        .await?;
                    self.store
                        .update_run_status(&run.id, RunStatus::Failed.as_str(), Some(&error_json))
                        .await?;
                    self.emit(
                        workspace_id,
                        &run.id,
                        "run.failed",
                        json!({ "error": err.to_string() }),
                    )
                    .await?;
                    return Err(err);
                }
            }
        }

        if interrupt.is_requested() {
            self.store
                .update_run_status(&run.id, RunStatus::Interrupted.as_str(), None)
                .await?;
            self.emit(workspace_id, &run.id, "run.interrupted", json!({}))
                .await?;
            return self.outcome(&run.id).await;
        }

        self.store
            .update_run_status(&run.id, RunStatus::Succeeded.as_str(), None)
            .await?;
        self.emit(workspace_id, &run.id, "run.succeeded", json!({}))
            .await?;
        self.outcome(&run.id).await
    }

    pub(crate) async fn ensure_run_steps(
        &self,
        run_id: &str,
        plan: &ExecutionPlan,
    ) -> RunResult<Vec<RunStepRecord>> {
        let existing = self.store.run_steps(run_id).await?;
        if !existing.is_empty() {
            return Ok(existing);
        }

        let mut steps = Vec::new();
        for step in &plan.steps {
            steps.push(
                self.store
                    .create_run_step(NewRunStep {
                        run_id,
                        node_id: &step.node_id,
                        node_type: &step.node_type,
                        provider: step.provider.as_deref(),
                        state: RunStepState::Queued.as_str(),
                    })
                    .await?,
            );
        }
        Ok(steps)
    }

    async fn execute_step(
        &self,
        workspace_id: &str,
        run_id: &str,
        step: &ExecutionStep,
        record: &RunStepRecord,
        outputs: &mut BTreeMap<[String; 2], ArtifactRef>,
        interrupt: &RunInterrupt,
    ) -> RunResult<StepExecution> {
        self.store
            .update_run_step_state(
                &record.id,
                RunStepState::Running.as_str(),
                Some(0.0),
                None,
                None,
            )
            .await?;
        self.emit_node_state(workspace_id, run_id, step, RunStepState::Running, None)
            .await?;

        let cost = if let (Some(provider), Some(capability)) = (&step.provider, &step.capability) {
            let request = ProviderRequest {
                provider: provider.clone(),
                capability: capability.clone(),
                node_id: step.node_id.clone(),
                run_id: run_id.to_owned(),
                inputs: resolve_inputs(&step.inputs, outputs)?,
                params: step.params.clone(),
            };
            let result = tokio::select! {
                result = self.provider.invoke(request) => result?,
                _ = interrupt.cancelled() => return Ok(StepExecution::Interrupted),
            };
            for (port, payload) in result.outputs {
                let artifact = self
                    .persist_artifact(workspace_id, run_id, &record.id, &step.node_id, payload)
                    .await?;
                outputs.insert(
                    [step.node_id.clone(), port],
                    ArtifactRef {
                        artifact_id: artifact.id,
                        storage_uri: artifact.storage_uri,
                    },
                );
            }
            Some(result.cost)
        } else {
            self.execute_builtin(workspace_id, run_id, step, record, outputs)
                .await?;
            None
        };

        if interrupt.is_requested() {
            return Ok(StepExecution::Interrupted);
        }

        let cost_json = cost.as_ref().map(serde_json::to_string).transpose()?;
        self.store
            .update_run_step_state(
                &record.id,
                RunStepState::Succeeded.as_str(),
                Some(1.0),
                cost_json.as_deref(),
                None,
            )
            .await?;
        self.emit_node_state(workspace_id, run_id, step, RunStepState::Succeeded, None)
            .await?;
        Ok(StepExecution::Succeeded)
    }

    async fn execute_builtin(
        &self,
        workspace_id: &str,
        run_id: &str,
        step: &ExecutionStep,
        record: &RunStepRecord,
        outputs: &mut BTreeMap<[String; 2], ArtifactRef>,
    ) -> RunResult<()> {
        match step.node_type.as_str() {
            "input.text" => {
                let text = step
                    .params
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RunError::MissingParam {
                        node_id: step.node_id.clone(),
                        param: "text".to_owned(),
                    })?;
                let artifact = self
                    .store
                    .create_artifact(NewArtifact {
                        workspace_id,
                        run_id: Some(run_id),
                        run_step_id: Some(&record.id),
                        node_id: Some(&step.node_id),
                        kind: "text",
                        storage_uri: &format!(
                            "workspace://inputs/{run_id}/{}/text.txt",
                            step.node_id
                        ),
                        sha256: None,
                        mime: Some("text/plain"),
                        width: None,
                        height: None,
                        duration_ms: None,
                        selected: false,
                        meta_json: Some(&serde_json::to_string(&json!({ "text": text }))?),
                    })
                    .await?;
                outputs.insert(
                    [step.node_id.clone(), "text".to_owned()],
                    ArtifactRef {
                        artifact_id: artifact.id,
                        storage_uri: artifact.storage_uri,
                    },
                );
            }
            "input.image" => {
                let storage_uri = step
                    .params
                    .get("storage_uri")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RunError::MissingParam {
                        node_id: step.node_id.clone(),
                        param: "storage_uri".to_owned(),
                    })?;
                let artifact = self
                    .store
                    .create_artifact(NewArtifact {
                        workspace_id,
                        run_id: Some(run_id),
                        run_step_id: Some(&record.id),
                        node_id: Some(&step.node_id),
                        kind: "image",
                        storage_uri,
                        sha256: None,
                        mime: None,
                        width: None,
                        height: None,
                        duration_ms: None,
                        selected: false,
                        meta_json: None,
                    })
                    .await?;
                outputs.insert(
                    [step.node_id.clone(), "image".to_owned()],
                    ArtifactRef {
                        artifact_id: artifact.id,
                        storage_uri: artifact.storage_uri,
                    },
                );
            }
            "output.save" => {
                let inputs = resolve_inputs(&step.inputs, outputs)?;
                let artifact_ref =
                    inputs
                        .get("artifact")
                        .ok_or_else(|| RunError::MissingInput {
                            node_id: step.node_id.clone(),
                            port: "artifact".to_owned(),
                        })?;
                let source = self.store.artifact(&artifact_ref.artifact_id).await?;
                self.store
                    .create_artifact(NewArtifact {
                        workspace_id,
                        run_id: Some(run_id),
                        run_step_id: Some(&record.id),
                        node_id: Some(&step.node_id),
                        kind: &source.kind,
                        storage_uri: &source.storage_uri,
                        sha256: source.sha256.as_deref(),
                        mime: source.mime.as_deref(),
                        width: source.width,
                        height: source.height,
                        duration_ms: source.duration_ms,
                        selected: true,
                        meta_json: source.meta_json.as_deref(),
                    })
                    .await?;
            }
            other => return Err(RunError::UnsupportedBuiltin(other.to_owned())),
        }

        Ok(())
    }

    async fn persist_artifact(
        &self,
        workspace_id: &str,
        run_id: &str,
        step_id: &str,
        node_id: &str,
        payload: helixflow_gateway::ArtifactPayload,
    ) -> RunResult<ArtifactRecord> {
        let storage_uri =
            persist_provider_artifact(&self.artifact_root, run_id, step_id, node_id, &payload)
                .await?;
        Ok(self
            .store
            .create_artifact(NewArtifact {
                workspace_id,
                run_id: Some(run_id),
                run_step_id: Some(step_id),
                node_id: Some(node_id),
                kind: artifact_kind_label(payload.kind),
                storage_uri: &storage_uri,
                sha256: None,
                mime: Some(&payload.mime),
                width: payload.width.map(i64::from),
                height: payload.height.map(i64::from),
                duration_ms: payload.duration_ms.map(i64::from),
                selected: false,
                meta_json: Some(&serde_json::to_string(&payload.meta)?),
            })
            .await?)
    }

    async fn skip_steps(
        &self,
        workspace_id: &str,
        run_id: &str,
        steps: &[RunStepRecord],
    ) -> RunResult<()> {
        for step in steps {
            self.store
                .update_run_step_state(&step.id, RunStepState::Skipped.as_str(), None, None, None)
                .await?;
            self.emit(
                workspace_id,
                run_id,
                "node.state",
                json!({
                    "node_id": step.node_id,
                    "node_type": step.node_type,
                    "state": RunStepState::Skipped.as_str()
                }),
            )
            .await?;
        }
        Ok(())
    }

    async fn emit_node_state(
        &self,
        workspace_id: &str,
        run_id: &str,
        step: &ExecutionStep,
        state: RunStepState,
        error: Option<&str>,
    ) -> RunResult<()> {
        let mut data = json!({
            "node_id": step.node_id,
            "node_type": step.node_type,
            "provider": step.provider,
            "state": state.as_str()
        });
        if let Some(error) = error
            && let Some(map) = data.as_object_mut()
        {
            map.insert("error".to_owned(), Value::String(error.to_owned()));
        }
        self.emit(workspace_id, run_id, "node.state", data).await
    }

    pub(crate) async fn emit(
        &self,
        workspace_id: &str,
        run_id: &str,
        ev: &str,
        data: Value,
    ) -> RunResult<()> {
        let data_json = serde_json::to_string(&data)?;
        let event = self.store.append_run_event(run_id, ev, &data_json).await?;
        let _ = self.events.publish(RunEventEnvelope {
            workspace_id: workspace_id.to_owned(),
            run_id: event.run_id,
            seq: event.seq,
            server_time: event.created_at,
            ev: event.ev,
            data,
        });
        Ok(())
    }

    pub(crate) async fn outcome(&self, run_id: &str) -> RunResult<RunOutcome> {
        Ok(RunOutcome {
            run: self.store.run(run_id).await?,
            steps: self.store.run_steps(run_id).await?,
            artifacts: self.store.run_artifacts(run_id).await?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepExecution {
    Succeeded,
    Interrupted,
}

fn resolve_inputs(
    input_edges: &BTreeMap<String, [String; 2]>,
    outputs: &BTreeMap<[String; 2], ArtifactRef>,
) -> RunResult<BTreeMap<String, ArtifactRef>> {
    let mut inputs = BTreeMap::new();
    for (port, source) in input_edges {
        let artifact = outputs
            .get(source)
            .ok_or_else(|| RunError::MissingInput {
                node_id: source[0].clone(),
                port: source[1].clone(),
            })?
            .clone();
        inputs.insert(port.clone(), artifact);
    }
    Ok(inputs)
}

fn artifact_kind_label(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Text => "text",
        ArtifactKind::Image => "image",
        ArtifactKind::Video => "video",
        ArtifactKind::Json => "json",
    }
}

#[cfg(test)]
mod tests;
