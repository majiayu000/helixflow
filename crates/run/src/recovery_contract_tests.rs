use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use helixflow_gateway::{
    CostEstimate, DurableProviderTask, MockProvider, Provider, ProviderCatalog, ProviderDispatch,
    ProviderDispatchResult, ProviderHealth, ProviderRecoveryCapabilities, ProviderRequest,
    ProviderResult, ProviderResultValue, ProviderResume, ProviderTaskHandle,
};
use helixflow_store::{NewVersion, Store, VersionSource};
use tokio::sync::Semaphore;

use super::{ManualRunRequest, RunService};

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
