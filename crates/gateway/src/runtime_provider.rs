use async_trait::async_trait;

use crate::{
    ApiConnectorSummary, AtlasProvider, CostEstimate, FalProvider, MockProvider, Provider,
    ProviderCatalog, ProviderCatalogSnapshot, ProviderError, ProviderHealth, ProviderRequest,
    ProviderResult, ProviderResultValue, ProviderTaskHandle, RuntimeProviderSummary,
    WorkflowBackendSummary, is_safe_provider_message, safe_provider_message, sanitize_provider_id,
};

#[derive(Debug, Clone)]
pub enum RuntimeProvider {
    Mock(MockProvider),
    Atlas(AtlasProvider),
    Fal(FalProvider),
    Unavailable(UnavailableProvider),
}

impl RuntimeProvider {
    pub fn mock() -> Self {
        Self::Mock(MockProvider::new())
    }

    pub fn unavailable(id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Unavailable(UnavailableProvider::new(id, reason))
    }

    pub fn atlas_from_env() -> Self {
        AtlasProvider::from_env()
            .map(Self::Atlas)
            .unwrap_or_else(|| Self::unavailable("atlas", "Atlas provider is not configured"))
    }

    pub fn fal_from_env() -> Self {
        FalProvider::from_env()
            .map(Self::Fal)
            .unwrap_or_else(|| Self::unavailable("fal", "FAL_KEY is not configured"))
    }

    pub fn safe_id(&self) -> String {
        match self {
            Self::Mock(provider) => provider.id().to_owned(),
            Self::Atlas(provider) => provider.id().to_owned(),
            Self::Fal(provider) => provider.id().to_owned(),
            Self::Unavailable(provider) => provider.safe_id(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        !matches!(self, Self::Unavailable(_))
    }

    pub fn catalog_snapshot(&self) -> ProviderCatalogSnapshot {
        match self {
            Self::Mock(_) => mock_catalog_snapshot(),
            Self::Atlas(provider) => provider_catalog_snapshot(
                provider.id(),
                "Atlas API",
                "external_api",
                true,
                "healthy",
                Some("Atlas API provider configured".to_owned()),
                AtlasProvider::catalog_value(),
            ),
            Self::Fal(provider) => provider_catalog_snapshot(
                provider.id(),
                "fal.ai",
                "external_api",
                true,
                "healthy",
                Some("fal.ai provider configured".to_owned()),
                FalProvider::catalog_value(),
            ),
            Self::Unavailable(provider) => ProviderCatalogSnapshot {
                default_provider: provider.safe_id(),
                runtime_providers: vec![RuntimeProviderSummary {
                    id: provider.safe_id(),
                    label: provider.safe_id(),
                    kind: "unavailable".to_owned(),
                    enabled: false,
                    status: "unavailable".to_owned(),
                    message: Some(provider.safe_reason()),
                    capabilities: Vec::new(),
                }],
                workflow_backends: Vec::new(),
                api_connectors: Vec::new(),
            },
        }
    }
}

fn mock_catalog_snapshot() -> ProviderCatalogSnapshot {
    let catalog = MockProvider::catalog_value();
    let provider_id = catalog.provider.clone();
    provider_catalog_snapshot(
        &provider_id,
        "Mock (local test)",
        "local_test",
        true,
        "healthy",
        Some("non-production synthetic mock provider enabled for local testing".to_owned()),
        catalog,
    )
}

pub(crate) fn provider_catalog_snapshot(
    provider_id: &str,
    label: &str,
    kind: &str,
    enabled: bool,
    status: &str,
    message: Option<String>,
    catalog: ProviderCatalog,
) -> ProviderCatalogSnapshot {
    let capabilities = catalog.capabilities.keys().cloned().collect::<Vec<_>>();
    ProviderCatalogSnapshot {
        default_provider: provider_id.to_owned(),
        runtime_providers: vec![RuntimeProviderSummary {
            id: provider_id.to_owned(),
            label: label.to_owned(),
            kind: kind.to_owned(),
            enabled,
            status: status.to_owned(),
            message,
            capabilities: capabilities.clone(),
        }],
        workflow_backends: enabled
            .then(|| WorkflowBackendSummary {
                id: "helixflow_graph".to_owned(),
                label: "Helixflow Graph".to_owned(),
                status: "healthy".to_owned(),
            })
            .into_iter()
            .collect(),
        api_connectors: capabilities
            .into_iter()
            .map(|capability| ApiConnectorSummary {
                id: format!("{}.{}", provider_id, capability),
                provider: provider_id.to_owned(),
                capability,
                status: status.to_owned(),
            })
            .collect(),
    }
}

#[async_trait]
impl Provider for RuntimeProvider {
    fn id(&self) -> &str {
        match self {
            Self::Mock(provider) => provider.id(),
            Self::Atlas(provider) => provider.id(),
            Self::Fal(provider) => provider.id(),
            Self::Unavailable(provider) => provider.id(),
        }
    }

    async fn health(&self) -> ProviderHealth {
        match self {
            Self::Mock(provider) => provider.health().await,
            Self::Atlas(provider) => provider.health().await,
            Self::Fal(provider) => provider.health().await,
            Self::Unavailable(provider) => provider.health().await,
        }
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog> {
        match self {
            Self::Mock(provider) => provider.catalog().await,
            Self::Atlas(provider) => provider.catalog().await,
            Self::Fal(provider) => provider.catalog().await,
            Self::Unavailable(provider) => provider.catalog().await,
        }
    }

    async fn estimate(&self, req: ProviderRequest) -> ProviderResultValue<CostEstimate> {
        match self {
            Self::Mock(provider) => provider.estimate(req).await,
            Self::Atlas(provider) => provider.estimate(req).await,
            Self::Fal(provider) => provider.estimate(req).await,
            Self::Unavailable(provider) => provider.estimate(req).await,
        }
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        match self {
            Self::Mock(provider) => provider.invoke(req).await,
            Self::Atlas(provider) => provider.invoke(req).await,
            Self::Fal(provider) => provider.invoke(req).await,
            Self::Unavailable(provider) => provider.invoke(req).await,
        }
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        match self {
            Self::Mock(provider) => provider.cancel(handle).await,
            Self::Atlas(provider) => provider.cancel(handle).await,
            Self::Fal(provider) => provider.cancel(handle).await,
            Self::Unavailable(provider) => provider.cancel(handle).await,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnavailableProvider {
    id: String,
    reason: String,
}

impl UnavailableProvider {
    pub fn new(id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            reason: reason.into(),
        }
    }

    fn safe_id(&self) -> String {
        sanitize_provider_id(&self.id)
    }

    fn safe_reason(&self) -> String {
        if is_safe_provider_message(&self.reason) && self.id == self.safe_id() {
            return self.reason.clone();
        }
        safe_provider_message(&format!(
            "runtime provider `{}` is unavailable",
            self.safe_id()
        ))
    }

    fn unavailable<T>(&self) -> ProviderResultValue<T> {
        Err(ProviderError::Unavailable {
            provider: self.safe_id(),
            reason: self.safe_reason(),
        })
    }
}

#[async_trait]
impl Provider for UnavailableProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn health(&self) -> ProviderHealth {
        ProviderHealth {
            ok: false,
            message: Some(self.safe_reason()),
        }
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog> {
        self.unavailable()
    }

    async fn estimate(&self, _req: ProviderRequest) -> ProviderResultValue<CostEstimate> {
        self.unavailable()
    }

    async fn invoke(&self, _req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        self.unavailable()
    }

    async fn cancel(&self, _handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        self.unavailable()
    }
}
