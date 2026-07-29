use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::{error::Error, fmt};

use async_trait::async_trait;
use helixflow_agent::{
    AgentError, AgentService, AgentSessionRequest, CodexRuntime, ValidatedAgentIntent,
    ValidatedAgentProposal, ValidatedAgentReply,
};
#[cfg(test)]
use helixflow_gateway::RuntimeProvider;
use helixflow_gateway::{ProviderCatalogSnapshot, ProviderRegistry};
use helixflow_run::{
    DEFAULT_MAX_PARALLEL_STEPS, EventBus, RunError, RunService, normalize_max_parallel_steps,
};
use helixflow_store::{Store, StoreError, WorkspaceRecord};
use tokio::sync::Mutex;

use crate::version_file_reconciliation::{
    ReconciliationReport, VersionFileReconciliationError, reconcile_version_files,
};

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) events: EventBus,
    pub(crate) agent: Arc<dyn WorkbenchAgent>,
    pub(crate) agent_sessions_dir: PathBuf,
    pub(crate) store: Store,
    pub(crate) data_dir: PathBuf,
    pub(crate) provider_registry: ProviderRegistry,
    pub(crate) runner: RunService<ProviderRegistry>,
    pub(crate) run_queue_locks: Arc<Mutex<BTreeMap<String, Arc<Mutex<()>>>>>,
    pub(crate) reconciliation_report: Arc<ReconciliationReport>,
    /// GH130 T6: IntentPlan contract switch. Production reads the env flag
    /// (default on); test states default to the legacy path so proposal
    /// mechanics stay deterministically covered, and intent tests opt in.
    pub(crate) use_intent_contract: bool,
    pub(crate) migration_apply_enabled: bool,
}

impl AppState {
    pub(crate) async fn open(events: EventBus) -> Result<Self, AppStateError> {
        let data_dir = default_data_dir();
        eprintln!("helixflow data dir: {}", data_dir.display());
        let database_url = default_database_url(&data_dir);
        Self::open_in_data_dir(events, data_dir, database_url).await
    }

    async fn open_in_data_dir(
        events: EventBus,
        data_dir: PathBuf,
        database_url: String,
    ) -> Result<Self, AppStateError> {
        tokio::fs::create_dir_all(&data_dir).await?;
        let store = Store::open(&database_url).await?;
        let reconciliation_report = Arc::new(reconcile_version_files(&store, &data_dir).await?);
        let registry = default_provider_registry();
        persist_runtime_provider_status(&store, &registry).await?;
        let state =
            Self::with_store_provider(events, store, data_dir, registry, reconciliation_report);
        state.runner.validate_restart_config()?;
        let recovery_runner = state.runner.clone();
        tokio::spawn(async move {
            if let Err(err) = recovery_runner.recover_after_restart().await {
                eprintln!(
                    "run recovery startup worker failed: {}",
                    err.public_message()
                );
            }
        });
        Ok(state)
    }

    #[cfg(test)]
    pub(crate) async fn open_for_test(
        events: EventBus,
        data_dir: PathBuf,
    ) -> Result<Self, AppStateError> {
        let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
        Self::open_in_data_dir(events, data_dir, database_url).await
    }

    fn with_store_provider(
        events: EventBus,
        store: Store,
        data_dir: PathBuf,
        provider_registry: ProviderRegistry,
        reconciliation_report: Arc<ReconciliationReport>,
    ) -> Self {
        let agent_sessions_dir = default_agent_sessions_dir();
        let agent = Arc::new(CodexWorkbenchAgent {
            program: default_codex_program(),
            events: events.clone(),
        });
        let runner = RunService::with_provider_events_and_artifact_root(
            store.clone(),
            provider_registry.clone(),
            events.clone(),
            data_dir.clone(),
        )
        .with_max_parallel_steps(default_max_parallel_steps());
        let run_queue_locks = Arc::new(Mutex::new(BTreeMap::new()));
        Self {
            events,
            agent,
            agent_sessions_dir,
            store,
            data_dir,
            provider_registry,
            runner,
            run_queue_locks,
            reconciliation_report,
            use_intent_contract: crate::workbench_message_intent::intent_contract_enabled(),
            migration_apply_enabled: version_migration_apply_enabled(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_store_agent(
        events: EventBus,
        store: Store,
        data_dir: PathBuf,
        agent: Arc<dyn WorkbenchAgent>,
        agent_sessions_dir: PathBuf,
    ) -> Self {
        let provider_registry = ProviderRegistry::new("mock", vec![RuntimeProvider::mock()]);
        let runner = RunService::with_provider_events_and_artifact_root(
            store.clone(),
            provider_registry.clone(),
            events.clone(),
            data_dir.clone(),
        )
        .with_max_parallel_steps(default_max_parallel_steps());
        let run_queue_locks = Arc::new(Mutex::new(BTreeMap::new()));
        let reconciliation_report = Arc::new(ReconciliationReport::default());
        Self {
            events,
            agent,
            agent_sessions_dir,
            runner,
            store,
            data_dir,
            provider_registry,
            run_queue_locks,
            reconciliation_report,
            use_intent_contract: false,
            migration_apply_enabled: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_store_agent_provider(
        events: EventBus,
        store: Store,
        data_dir: PathBuf,
        agent: Arc<dyn WorkbenchAgent>,
        agent_sessions_dir: PathBuf,
        provider: RuntimeProvider,
    ) -> Self {
        let provider_registry = ProviderRegistry::new(provider.safe_id(), vec![provider]);
        let runner = RunService::with_provider_events_and_artifact_root(
            store.clone(),
            provider_registry.clone(),
            events.clone(),
            data_dir.clone(),
        )
        .with_max_parallel_steps(default_max_parallel_steps());
        let run_queue_locks = Arc::new(Mutex::new(BTreeMap::new()));
        let reconciliation_report = Arc::new(ReconciliationReport::default());
        Self {
            events,
            agent,
            agent_sessions_dir,
            runner,
            store,
            data_dir,
            provider_registry,
            run_queue_locks,
            reconciliation_report,
            use_intent_contract: false,
            migration_apply_enabled: false,
        }
    }

    pub(crate) fn selected_provider_for_workspace(&self, workspace: &WorkspaceRecord) -> String {
        self.provider_registry
            .selected_provider(workspace.runtime_provider_id.as_deref())
    }

    pub(crate) fn provider_catalog_for_workspace(
        &self,
        workspace: &WorkspaceRecord,
    ) -> ProviderCatalogSnapshot {
        let selected_provider = self.selected_provider_for_workspace(workspace);
        self.provider_registry
            .catalog_snapshot_for_selected(Some(&selected_provider))
    }
}

fn version_migration_apply_enabled() -> bool {
    std::env::var("HELIXFLOW_V1_MIGRATION_APPLY")
        .ok()
        .is_some_and(|value| matches!(value.trim(), "1" | "true" | "TRUE"))
}

#[derive(Debug)]
pub(crate) enum AppStateError {
    Io(std::io::Error),
    Store(StoreError),
    Run(RunError),
    VersionFileConsistency(VersionFileReconciliationError),
}

impl fmt::Display for AppStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "failed to initialize app data directory: {err}"),
            Self::Store(err) => write!(f, "failed to initialize store: {err}"),
            Self::Run(err) => write!(f, "failed to recover runs: {err}"),
            Self::VersionFileConsistency(err) => write!(f, "{err}"),
        }
    }
}

impl Error for AppStateError {}

impl From<std::io::Error> for AppStateError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<StoreError> for AppStateError {
    fn from(err: StoreError) -> Self {
        Self::Store(err)
    }
}

impl From<RunError> for AppStateError {
    fn from(err: RunError) -> Self {
        Self::Run(err)
    }
}

impl From<VersionFileReconciliationError> for AppStateError {
    fn from(err: VersionFileReconciliationError) -> Self {
        Self::VersionFileConsistency(err)
    }
}

#[async_trait]
pub(crate) trait WorkbenchAgent: Send + Sync {
    async fn answer_chat(
        &self,
        request: AgentSessionRequest,
    ) -> Result<ValidatedAgentReply, AgentError>;

    async fn propose_graph_change(
        &self,
        request: AgentSessionRequest,
    ) -> Result<ValidatedAgentProposal, AgentError>;

    /// GH130 T6: IntentPlan contract. Test doubles that never exercise the
    /// intent path keep this default, which fails closed instead of
    /// pretending to produce an intent.
    async fn propose_intent(
        &self,
        request: AgentSessionRequest,
    ) -> Result<ValidatedAgentIntent, AgentError> {
        Err(AgentError::InvalidMode {
            mode: request.mode,
            expected: helixflow_agent::OutputContract::IntentJson,
            actual: request
                .mode
                .output_contract_with(request.use_intent_contract),
        })
    }
}

struct CodexWorkbenchAgent {
    program: PathBuf,
    events: EventBus,
}

#[async_trait]
impl WorkbenchAgent for CodexWorkbenchAgent {
    async fn answer_chat(
        &self,
        request: AgentSessionRequest,
    ) -> Result<ValidatedAgentReply, AgentError> {
        AgentService::new(CodexRuntime::new(self.program.clone()), self.events.clone())
            .answer_chat(request)
            .await
    }

    async fn propose_graph_change(
        &self,
        request: AgentSessionRequest,
    ) -> Result<ValidatedAgentProposal, AgentError> {
        AgentService::new(CodexRuntime::new(self.program.clone()), self.events.clone())
            .propose_graph_change(request)
            .await
    }

    async fn propose_intent(
        &self,
        request: AgentSessionRequest,
    ) -> Result<ValidatedAgentIntent, AgentError> {
        AgentService::new(CodexRuntime::new(self.program.clone()), self.events.clone())
            .propose_intent(request)
            .await
    }
}

fn default_agent_sessions_dir() -> PathBuf {
    std::env::var_os("HELIXFLOW_AGENT_SESSIONS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("helixflow_agent_sessions"))
}

fn default_codex_program() -> PathBuf {
    std::env::var_os("HELIXFLOW_CODEX_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("codex"))
}

fn default_provider_registry() -> ProviderRegistry {
    ProviderRegistry::from_env()
}

fn default_max_parallel_steps() -> usize {
    parse_max_parallel_steps(
        std::env::var("HELIXFLOW_MAX_PARALLEL_STEPS")
            .ok()
            .as_deref(),
    )
}

fn parse_max_parallel_steps(raw: Option<&str>) -> usize {
    raw.and_then(|value| value.parse::<usize>().ok())
        .map(normalize_max_parallel_steps)
        .unwrap_or(DEFAULT_MAX_PARALLEL_STEPS)
}

async fn persist_runtime_provider_status(
    store: &Store,
    registry: &ProviderRegistry,
) -> Result<(), AppStateError> {
    for provider in registry.catalog_snapshot().runtime_providers {
        store
            .upsert_provider_status(&provider.id, provider.enabled, &provider.status, None, None)
            .await?;
    }
    Ok(())
}

fn default_data_dir() -> PathBuf {
    // A stable per-user location instead of the process cwd, so starting
    // the server from a different directory no longer creates a second
    // empty database (HF-032).
    std::env::var_os("HELIXFLOW_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|home| home.join(".helixflow")))
        .unwrap_or_else(|| std::env::temp_dir().join(".helixflow"))
}

fn default_database_url(data_dir: &std::path::Path) -> String {
    std::env::var("HELIXFLOW_DATABASE_URL")
        .unwrap_or_else(|_| format!("sqlite://{}", data_dir.join("helixflow.sqlite").display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use helixflow_store::{NewRun, NewRunStep, NewVersion, Store, VersionSource};

    #[test]
    fn provider_registry_keeps_mock_default() {
        let registry = ProviderRegistry::new("mock", vec![RuntimeProvider::mock()]);

        assert_eq!(registry.default_provider(), "mock");
        assert!(registry.provider_enabled("mock"));
    }

    #[test]
    fn provider_registry_records_unconfigured_default() {
        let registry = ProviderRegistry::new("openai", vec![RuntimeProvider::mock()]);

        assert_eq!(registry.default_provider(), "openai");
        assert!(!registry.provider_enabled("openai"));
    }

    #[tokio::test]
    async fn workspace_without_persisted_provider_inherits_fail_closed_server_default() {
        let dir = tempfile::tempdir().expect("temp dir");
        let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store
            .create_workspace("Fail-closed workspace")
            .await
            .expect("create workspace");
        let registry = ProviderRegistry::new(
            "unconfigured",
            vec![RuntimeProvider::unavailable(
                "unconfigured",
                "HELIXFLOW_RUNTIME_PROVIDER is not configured",
            )],
        );
        let state = AppState::with_store_provider(
            EventBus::new(16),
            store,
            dir.path().to_path_buf(),
            registry,
            Arc::new(ReconciliationReport::default()),
        );

        let selected = state.selected_provider_for_workspace(&workspace);
        let catalog = state.provider_catalog_for_workspace(&workspace);

        assert_eq!(selected, "unconfigured");
        assert_eq!(catalog.default_provider, "unconfigured");
        assert_eq!(catalog.runtime_providers.len(), 1);
        assert_eq!(catalog.runtime_providers[0].kind, "unavailable");
        assert!(!catalog.runtime_providers[0].enabled);
    }

    #[test]
    fn provider_catalog_can_include_unavailable_fal_with_mock() {
        let registry = ProviderRegistry::new(
            "mock",
            vec![
                RuntimeProvider::mock(),
                RuntimeProvider::unavailable("fal", "FAL_KEY is not configured"),
            ],
        );

        let snapshot = registry.catalog_snapshot_for_selected(Some("fal"));
        let fal = snapshot
            .runtime_providers
            .iter()
            .find(|provider| provider.id == "fal")
            .map(|provider| {
                (
                    provider.enabled,
                    provider.status.as_str(),
                    provider.message.as_deref(),
                )
            });
        let mock_enabled = snapshot
            .runtime_providers
            .iter()
            .find(|provider| provider.id == "mock")
            .map(|provider| provider.enabled);

        assert_eq!(mock_enabled, Some(true));
        assert_eq!(
            fal,
            Some((false, "unavailable", Some("FAL_KEY is not configured")))
        );
        assert!(
            snapshot
                .api_connectors
                .iter()
                .any(|item| item.provider == "mock")
        );
    }

    #[test]
    fn max_parallel_steps_config_uses_safe_default_and_normalization() {
        assert_eq!(parse_max_parallel_steps(None), DEFAULT_MAX_PARALLEL_STEPS);
        assert_eq!(parse_max_parallel_steps(Some("4")), 4);
        assert_eq!(parse_max_parallel_steps(Some("0")), 1);
        assert_eq!(
            parse_max_parallel_steps(Some("not-a-number")),
            DEFAULT_MAX_PARALLEL_STEPS
        );
    }

    #[tokio::test]
    async fn persist_runtime_provider_status_marks_unavailable_provider() {
        let dir = tempfile::tempdir().expect("temp dir");
        let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let registry = ProviderRegistry::new(
            "openai",
            vec![RuntimeProvider::unavailable(
                "openai",
                "missing connector config",
            )],
        );

        persist_runtime_provider_status(&store, &registry)
            .await
            .expect("persist provider status");

        let record = store
            .provider_status("openai")
            .await
            .expect("provider status row");

        assert_eq!(record.id, "openai");
        assert!(!record.enabled);
        assert_eq!(record.status, "unavailable");
    }

    #[tokio::test]
    async fn persist_runtime_provider_status_uses_safe_provider_id() {
        let dir = tempfile::tempdir().expect("temp dir");
        let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let registry = ProviderRegistry::new(
            "sk-secret-token",
            vec![RuntimeProvider::unavailable(
                "sk-secret-token",
                "runtime provider `sk-secret-token` is not configured",
            )],
        );

        persist_runtime_provider_status(&store, &registry)
            .await
            .expect("persist provider status");

        let record = store
            .provider_status("invalid")
            .await
            .expect("safe provider status row");

        assert_eq!(record.id, "invalid");
        assert!(!record.enabled);
        assert_eq!(record.status, "unavailable");
    }

    #[tokio::test]
    async fn startup_cleanup_interrupts_active_runs_and_records_events() {
        let dir = tempfile::tempdir().expect("temp dir");
        let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store
            .create_workspace("Restart cleanup")
            .await
            .expect("create workspace");
        let version = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Graph",
                source: VersionSource::Manual,
                graph_path: "graphs/current.json",
                graph_hash: "sha256:graph",
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
                label: "Stale run",
                trigger: "manual",
                plan_json: None,
                estimate_json: None,
                status: "running",
            })
            .await
            .expect("create run");
        store
            .create_run_step(NewRunStep {
                run_id: &run.id,
                node_id: "node",
                node_type: "input.text",
                provider: None,
                state: "running",
            })
            .await
            .expect("create step");
        let events = EventBus::new(16);

        RunService::with_provider_and_events(store.clone(), RuntimeProvider::mock(), events)
            .recover_after_restart()
            .await
            .expect("recover active runs");

        let run = store.run(&run.id).await.expect("run");
        let steps = store.run_steps(&run.id).await.expect("steps");
        let stored_events = store.run_events(&run.id).await.expect("events");
        assert_eq!(run.status, "interrupted");
        assert_eq!(steps[0].state, "skipped");
        assert_eq!(stored_events[0].ev, "run.interrupted");
        assert_eq!(stored_events[0].data_json, "{}");
    }
}
