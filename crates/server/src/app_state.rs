use std::path::PathBuf;
use std::sync::Arc;
use std::{error::Error, fmt};

use async_trait::async_trait;
use helixflow_agent::{
    AgentError, AgentService, AgentSessionRequest, CodexRuntime, ValidatedAgentProposal,
    ValidatedAgentReply,
};
use helixflow_gateway::{Provider, RuntimeProvider};
use helixflow_run::{EventBus, RunService};
use helixflow_store::{Store, StoreError};

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) events: EventBus,
    pub(crate) agent: Arc<dyn WorkbenchAgent>,
    pub(crate) agent_sessions_dir: PathBuf,
    pub(crate) store: Store,
    pub(crate) data_dir: PathBuf,
    pub(crate) runner: RunService<RuntimeProvider>,
}

impl AppState {
    pub(crate) async fn open(events: EventBus) -> Result<Self, AppStateError> {
        let data_dir = default_data_dir();
        tokio::fs::create_dir_all(&data_dir).await?;
        let database_url = default_database_url(&data_dir);
        let store = Store::open(&database_url).await?;
        let provider = default_runtime_provider();
        persist_runtime_provider_status(&store, &provider).await?;
        Ok(Self::with_store_provider(events, store, data_dir, provider))
    }

    fn with_store_provider(
        events: EventBus,
        store: Store,
        data_dir: PathBuf,
        provider: RuntimeProvider,
    ) -> Self {
        let agent_sessions_dir = default_agent_sessions_dir();
        let agent = Arc::new(CodexWorkbenchAgent {
            program: default_codex_program(),
            events: events.clone(),
        });
        let runner = RunService::with_provider_and_events(store.clone(), provider, events.clone());
        Self {
            events,
            agent,
            agent_sessions_dir,
            store,
            data_dir,
            runner,
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
        let runner = RunService::with_provider_and_events(
            store.clone(),
            RuntimeProvider::mock(),
            events.clone(),
        );
        Self {
            events,
            agent,
            agent_sessions_dir,
            runner,
            store,
            data_dir,
        }
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

fn default_runtime_provider() -> RuntimeProvider {
    match std::env::var("HELIXFLOW_RUNTIME_PROVIDER") {
        Ok(provider_id) => runtime_provider_from_id(provider_id.trim()),
        Err(std::env::VarError::NotPresent) => RuntimeProvider::mock(),
        Err(std::env::VarError::NotUnicode(_)) => RuntimeProvider::unavailable(
            "invalid",
            "HELIXFLOW_RUNTIME_PROVIDER is not valid unicode",
        ),
    }
}

fn runtime_provider_from_id(provider_id: &str) -> RuntimeProvider {
    match provider_id {
        "mock" => RuntimeProvider::mock(),
        "" => RuntimeProvider::unavailable("invalid", "HELIXFLOW_RUNTIME_PROVIDER is empty"),
        other => RuntimeProvider::unavailable(
            other,
            format!("runtime provider `{other}` is not configured by this build"),
        ),
    }
}

async fn persist_runtime_provider_status(
    store: &Store,
    provider: &RuntimeProvider,
) -> Result<(), AppStateError> {
    let health = provider.health().await;
    let status = if health.ok { "healthy" } else { "unavailable" };
    store
        .upsert_provider_status(provider.id(), health.ok, status, None, None)
        .await?;
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
    fn runtime_provider_from_id_keeps_mock_explicit() {
        let provider = runtime_provider_from_id("mock");

        assert_eq!(provider.id(), "mock");
        assert!(matches!(provider, RuntimeProvider::Mock(_)));
    }

    #[test]
    fn runtime_provider_from_id_rejects_unconfigured_provider() {
        let provider = runtime_provider_from_id("openai");

        assert_eq!(provider.id(), "openai");
        assert!(matches!(provider, RuntimeProvider::Unavailable(_)));
    }

    #[tokio::test]
    async fn persist_runtime_provider_status_marks_unavailable_provider() {
        let dir = tempfile::tempdir().expect("temp dir");
        let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let provider = RuntimeProvider::unavailable("openai", "missing connector config");

        persist_runtime_provider_status(&store, &provider)
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
}
