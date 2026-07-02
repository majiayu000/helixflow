use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::{error::Error, fmt};

use async_trait::async_trait;
use helixflow_agent::{
    AgentError, AgentService, AgentSessionRequest, CodexRuntime, ValidatedAgentProposal,
    ValidatedAgentReply,
};
#[cfg(test)]
use helixflow_gateway::RuntimeProvider;
use helixflow_gateway::{ProviderCatalogSnapshot, ProviderRegistry};
use helixflow_run::{EventBus, RunService};
use helixflow_store::{Store, StoreError, WorkspaceRecord};
use tokio::sync::Mutex;

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
}

impl AppState {
    pub(crate) async fn open(events: EventBus) -> Result<Self, AppStateError> {
        let data_dir = default_data_dir();
        tokio::fs::create_dir_all(&data_dir).await?;
        let database_url = default_database_url(&data_dir);
        let store = Store::open(&database_url).await?;
        let registry = default_provider_registry();
        persist_runtime_provider_status(&store, &registry).await?;
        Ok(Self::with_store_provider(events, store, data_dir, registry))
    }

    fn with_store_provider(
        events: EventBus,
        store: Store,
        data_dir: PathBuf,
        provider_registry: ProviderRegistry,
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
        );
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
        );
        let run_queue_locks = Arc::new(Mutex::new(BTreeMap::new()));
        Self {
            events,
            agent,
            agent_sessions_dir,
            runner,
            store,
            data_dir,
            provider_registry,
            run_queue_locks,
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
        );
        let run_queue_locks = Arc::new(Mutex::new(BTreeMap::new()));
        Self {
            events,
            agent,
            agent_sessions_dir,
            runner,
            store,
            data_dir,
            provider_registry,
            run_queue_locks,
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

#[derive(Debug)]
pub(crate) enum AppStateError {
    Io(std::io::Error),
    Store(StoreError),
}

impl fmt::Display for AppStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "failed to initialize app data directory: {err}"),
            Self::Store(err) => write!(f, "failed to initialize store: {err}"),
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
    std::env::var_os("HELIXFLOW_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_else(|_| std::env::temp_dir())
                .join(".helixflow")
        })
}

fn default_database_url(data_dir: &std::path::Path) -> String {
    std::env::var("HELIXFLOW_DATABASE_URL")
        .unwrap_or_else(|_| format!("sqlite://{}", data_dir.join("helixflow.sqlite").display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use helixflow_store::Store;

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
}
