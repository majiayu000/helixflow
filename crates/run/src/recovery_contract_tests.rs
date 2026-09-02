use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use helixflow_gateway::{
    CostEstimate, DurableProviderTask, MockProvider, Provider, ProviderCatalog, ProviderDispatch,
    ProviderDispatchResult, ProviderHealth, ProviderRecoveryCapabilities, ProviderRequest,
    ProviderResult, ProviderResultValue, ProviderResume, ProviderTaskHandle,
};
use helixflow_store::{
    NewArtifact, NewArtifactPublishJournal, NewProviderTask, NewRun, NewRunStep, NewVersion,
    ProviderTaskResult, Store, VersionSource,
};
use tokio::sync::Semaphore;

use super::{EventBus, ManualRunRequest, RunService};

#[derive(Clone)]
struct DelayedDispatchProvider {
    inner: MockProvider,
    dispatch_started: Arc<Semaphore>,
    release_dispatch: Arc<Semaphore>,
    cancelled: Arc<AtomicUsize>,
}

impl Default for DelayedDispatchProvider {
    fn default() -> Self {
        Self {
            inner: MockProvider::new(),
            dispatch_started: Arc::new(Semaphore::new(0)),
            release_dispatch: Arc::new(Semaphore::new(0)),
            cancelled: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait]
impl Provider for DelayedDispatchProvider {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn recovery_capabilities(&self, _provider_id: &str) -> ProviderRecoveryCapabilities {
        ProviderRecoveryCapabilities {
            resume: true,
            cancel: true,
        }
    }

    async fn health(&self) -> ProviderHealth {
        self.inner.health().await
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog> {
        self.inner.catalog().await
    }

    async fn estimate(&self, req: ProviderRequest) -> ProviderResultValue<CostEstimate> {
        self.inner.estimate(req).await
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        self.inner.invoke(req).await
    }

    async fn dispatch(&self, req: ProviderRequest) -> ProviderDispatchResult {
        self.dispatch_started.add_permits(1);
        let permit = self
            .release_dispatch
            .acquire()
            .await
            .expect("dispatch release semaphore stays open");
        permit.forget();
        Ok(ProviderDispatch::Accepted(DurableProviderTask {
            provider: req.provider,
            provider_task_id: format!("remote-{}", req.run_id),
            dispatch_origin: "provider://mock".to_owned(),
            recovery_scope_fingerprint: String::new(),
            status_url: None,
            result_url: None,
        }))
    }

    async fn resume(
        &self,
        _task: &DurableProviderTask,
        _req: &ProviderRequest,
    ) -> ProviderResultValue<ProviderResume> {
        Ok(ProviderResume::Pending { retry_after_ms: 25 })
    }

    async fn cancel(&self, _handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        self.cancelled.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn interrupt_completes_a_provider_result_waiting_for_materialization() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    let workspace = store
        .create_workspace("Materialization interrupt")
        .await
        .expect("create workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Materialization interrupt",
            source: VersionSource::Manual,
            graph_path: "graphs/materialization-interrupt.json",
            graph_hash: "sha256:materialization-interrupt",
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("create version");
    let run = store
        .create_run(NewRun {
            workspace_id: &workspace.id,
            version_id: &version.id,
            group_id: None,
            label: "Materialization interrupt",
            trigger: "manual",
            plan_json: None,
            estimate_json: None,
            status: "running",
        })
        .await
        .expect("create run");
    let step = store
        .create_run_step(NewRunStep {
            run_id: &run.id,
            node_id: "image",
            node_type: "image.generate",
            provider: Some("mock"),
            state: "running",
        })
        .await
        .expect("create step");
    let task = store
        .insert_or_read_provider_task(NewProviderTask {
            run_id: &run.id,
            run_step_id: &step.id,
            provider: "mock",
            dispatch_origin: "provider://mock",
            recovery_scope_fingerprint: "",
            operation_key: "dispatch:image",
            dispatch_owner_id: "owner",
            dispatch_lease_seconds: 30,
            dispatch_deadline_seconds: 60,
        })
        .await
        .expect("create provider task");
    store
        .mark_provider_task_result_ready(ProviderTaskResult {
            task_id: &task.id,
            dispatch_owner_id: Some("owner"),
            terminal_outcome: "succeeded",
            result_spool_path: "recovery_spool/result.json",
            result_fingerprint: "sha256:result",
            materialization_deadline_seconds: 3_600,
            workspace_id: &workspace.id,
            provider: "mock",
            amount: 0.0,
            currency: "USD",
            estimated: true,
        })
        .await
        .expect("mark result ready")
        .expect("provider task transition");
    let service = RunService::with_provider(store.clone(), MockProvider::new());

    tokio::time::timeout(Duration::from_secs(1), service.interrupt_run(&run.id))
        .await
        .expect("interrupt must not wait for the materialization deadline")
        .expect("interrupt run");

    assert_eq!(store.run(&run.id).await.expect("run").status, "interrupted");
    let task = store.provider_task(&task.id).await.expect("provider task");
    assert_eq!(task.state, "completed");
    assert_eq!(
        task.last_error_code.as_deref(),
        Some("ARTIFACT_MATERIALIZATION_INTERRUPTED")
    );
}

#[tokio::test]
async fn interrupt_waits_for_live_dispatch_then_persists_and_cancels_handle() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    let workspace = store
        .create_workspace("Dispatch recovery")
        .await
        .expect("create workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Dispatch recovery",
            source: VersionSource::Manual,
            graph_path: "graphs/dispatch-recovery.json",
            graph_hash: "sha256:dispatch-recovery",
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("create version");
    let provider = DelayedDispatchProvider::default();
    let service = RunService::with_provider(store.clone(), provider.clone());
    let pending = service
        .start_manual_run(ManualRunRequest {
            workspace_id: workspace.id,
            version_id: version.id,
            group_id: None,
            label: "Interrupt during dispatch".to_owned(),
            provider: "mock".to_owned(),
            graph: super::tests::executable_graph(),
            force_rerun: false,
        })
        .await
        .expect("start run");
    let permit = tokio::time::timeout(Duration::from_secs(2), provider.dispatch_started.acquire())
        .await
        .expect("dispatch starts")
        .expect("dispatch semaphore stays open");
    permit.forget();

    let interrupter = {
        let service = service.clone();
        let run_id = pending.run.id.clone();
        tokio::spawn(async move { service.interrupt_run(&run_id).await })
    };
    tokio::time::sleep(Duration::from_millis(25)).await;
    let before_release = store
        .provider_tasks_for_run(&pending.run.id)
        .await
        .expect("read dispatching task");
    assert_eq!(before_release[0].state, "dispatching");
    provider.release_dispatch.add_permits(1);
    interrupter
        .await
        .expect("join interrupter")
        .expect("interrupt run");

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let run = store.run(&pending.run.id).await.expect("read run");
        if run.status == "interrupted" {
            break;
        }
        assert!(Instant::now() < deadline, "run did not settle after cancel");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let tasks = store
        .provider_tasks_for_run(&pending.run.id)
        .await
        .expect("read durable task");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].state, "cancelled");
    assert!(tasks[0].provider_task_id.is_some());
    assert_eq!(provider.cancelled.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn expired_unreferenced_artifact_journal_is_garbage_collected() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    let workspace = store
        .create_workspace("Artifact journal GC")
        .await
        .expect("create workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Artifact journal GC",
            source: VersionSource::Manual,
            graph_path: "graphs/artifact-journal-gc.json",
            graph_hash: "sha256:artifact-journal-gc",
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("create version");
    let artifact_root = dir.path().join("run-artifacts");
    let service = RunService::with_provider_events_and_artifact_root(
        store.clone(),
        MockProvider::new(),
        EventBus::default(),
        &artifact_root,
    );
    let outcome = service
        .execute_manual_run(ManualRunRequest {
            workspace_id: workspace.id,
            version_id: version.id,
            group_id: None,
            label: "Create terminal run".to_owned(),
            provider: "mock".to_owned(),
            graph: super::tests::executable_graph(),
            force_rerun: false,
        })
        .await
        .expect("execute terminal run");
    let step_id = &outcome.steps[0].id;
    let orphan_relative = "artifacts/orphan-recovery.png";
    let orphan_path = artifact_root.join(orphan_relative);
    tokio::fs::create_dir_all(orphan_path.parent().expect("artifact parent"))
        .await
        .expect("create artifact parent");
    tokio::fs::write(&orphan_path, b"orphan")
        .await
        .expect("write orphan");
    store
        .create_or_read_artifact_publish_journal(NewArtifactPublishJournal {
            operation_key: "artifact:expired-orphan",
            run_id: &outcome.run.id,
            run_step_id: step_id,
            staged_path: orphan_relative,
            content_sha256: "sha256:orphan",
            owner_id: "stale-materializer",
            expires_after_seconds: 1,
        })
        .await
        .expect("create expiring journal");
    let referenced_relative = "artifacts/referenced-recovery.png";
    let referenced_path = artifact_root.join(referenced_relative);
    tokio::fs::write(&referenced_path, b"referenced")
        .await
        .expect("write referenced artifact");
    store
        .create_artifact(NewArtifact {
            workspace_id: &outcome.run.workspace_id,
            run_id: Some(&outcome.run.id),
            run_step_id: Some(step_id),
            node_id: None,
            kind: "image",
            storage_uri: referenced_relative,
            sha256: None,
            mime: Some("image/png"),
            width: None,
            height: None,
            duration_ms: None,
            selected: false,
            meta_json: None,
        })
        .await
        .expect("create referenced artifact");
    store
        .create_or_read_artifact_publish_journal(NewArtifactPublishJournal {
            operation_key: "artifact:referenced-file",
            run_id: &outcome.run.id,
            run_step_id: step_id,
            staged_path: referenced_relative,
            content_sha256: "sha256:referenced",
            owner_id: "stale-materializer",
            expires_after_seconds: 1,
        })
        .await
        .expect("create referenced journal");
    tokio::time::sleep(Duration::from_millis(1100)).await;
    service
        .reconcile_artifact_publish_journals()
        .await
        .expect("reconcile journals");
    assert!(!orphan_path.exists());
    assert!(referenced_path.exists());
    let pending = store
        .pending_artifact_publish_journals()
        .await
        .expect("read pending journals");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].operation_key, "artifact:referenced-file");
}
