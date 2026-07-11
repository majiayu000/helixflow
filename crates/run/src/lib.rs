use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use helixflow_gateway::{ArtifactRef, MockProvider, Provider};
use helixflow_graph::{ExecutionPlan, GraphService, WorkflowGraph};
use helixflow_registry::NodeRegistry;
use helixflow_store::{ArtifactRecord, NewRun, NewRunStep, RunRecord, RunStepRecord, Store};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex, Notify, broadcast};

mod artifacts;
mod background;
mod cache;
mod cost_gate;
mod cost_types;
mod error;
mod executor;
mod run_policy;
mod self_heal;
#[cfg(test)]
mod self_heal_tests;
mod sweep_background;

use artifacts::default_artifact_root;
pub use cost_types::{
    AgentRunRequest, CostSummary, PendingRun, PendingSweep, SweepOutcome, SweepPlan, SweepVariant,
};
pub use error::{RunError, RunResult};
pub use run_policy::{
    max_run_retries, parse_max_run_retries, parse_run_confirmation_threshold_usd,
    run_confirmation_threshold_usd, run_requires_confirmation,
};

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
    pub force_rerun: bool,
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
pub(crate) struct StepOutput {
    pub(crate) port: String,
    pub(crate) artifact: ArtifactRef,
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

    pub(crate) fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    pub(crate) async fn cancelled(&self) {
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
    max_parallel_steps: usize,
}

pub const DEFAULT_MAX_PARALLEL_STEPS: usize = 2;

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
            max_parallel_steps: DEFAULT_MAX_PARALLEL_STEPS,
        }
    }

    pub fn with_max_parallel_steps(mut self, max_parallel_steps: usize) -> Self {
        self.max_parallel_steps = normalize_max_parallel_steps(max_parallel_steps);
        self
    }

    pub fn max_parallel_steps(&self) -> usize {
        self.max_parallel_steps
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
            .execute_created_run(
                &run,
                &request.workspace_id,
                &plan,
                interrupt,
                request.force_rerun,
            )
            .await;
        self.interrupts.lock().await.remove(&run.id);
        result
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
}

pub fn normalize_max_parallel_steps(max_parallel_steps: usize) -> usize {
    max_parallel_steps.max(1)
}

#[cfg(test)]
mod background_tests;
#[cfg(test)]
mod cache_tests;
#[cfg(test)]
mod parallel_tests;
#[cfg(test)]
mod sweep_tests;
#[cfg(test)]
mod tests;
