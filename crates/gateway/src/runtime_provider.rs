use async_trait::async_trait;

use crate::{
    ApiConnectorSummary, CostEstimate, MockProvider, Provider, ProviderCatalog,
    ProviderCatalogSnapshot, ProviderError, ProviderHealth, ProviderRequest, ProviderResult,
    ProviderResultValue, ProviderTaskHandle, RuntimeProviderSummary, WorkflowBackendSummary,
};

#[derive(Debug, Clone)]
pub enum RuntimeProvider {
    Mock(MockProvider),
    Unavailable(UnavailableProvider),
}

impl RuntimeProvider {
    pub fn mock() -> Self {
        Self::Mock(MockProvider::new())
    }

    pub fn unavailable(id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Unavailable(UnavailableProvider::new(id, reason))
    }

    pub fn catalog_snapshot(&self) -> ProviderCatalogSnapshot {
        match self {
            Self::Mock(_) => mock_catalog_snapshot(),
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
    let capabilities = catalog.capabilities.keys().cloned().collect::<Vec<_>>();
    ProviderCatalogSnapshot {
        default_provider: catalog.provider.clone(),
        runtime_providers: vec![RuntimeProviderSummary {
            id: catalog.provider.clone(),
            label: "Mock Provider".to_owned(),
            kind: "local_test".to_owned(),
            enabled: true,
            status: "healthy".to_owned(),
            message: Some("mock provider ready".to_owned()),
            capabilities: capabilities.clone(),
        }],
        workflow_backends: vec![WorkflowBackendSummary {
            id: "helixflow_graph".to_owned(),
            label: "Helixflow Graph".to_owned(),
            status: "healthy".to_owned(),
        }],
        api_connectors: capabilities
            .into_iter()
            .map(|capability| ApiConnectorSummary {
                id: format!("{}.{}", catalog.provider, capability),
                provider: catalog.provider.clone(),
                capability,
                status: "healthy".to_owned(),
            })
            .collect(),
    }
}

#[async_trait]
impl Provider for RuntimeProvider {
    fn id(&self) -> &str {
        match self {
            Self::Mock(provider) => provider.id(),
            Self::Unavailable(provider) => provider.id(),
        }
    }

    async fn health(&self) -> ProviderHealth {
        match self {
            Self::Mock(provider) => provider.health().await,
            Self::Unavailable(provider) => provider.health().await,
        }
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog> {
        match self {
            Self::Mock(provider) => provider.catalog().await,
            Self::Unavailable(provider) => provider.catalog().await,
        }
    }

    async fn estimate(&self, req: ProviderRequest) -> ProviderResultValue<CostEstimate> {
        match self {
            Self::Mock(provider) => provider.estimate(req).await,
            Self::Unavailable(provider) => provider.estimate(req).await,
        }
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        match self {
            Self::Mock(provider) => provider.invoke(req).await,
            Self::Unavailable(provider) => provider.invoke(req).await,
        }
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        match self {
            Self::Mock(provider) => provider.cancel(handle).await,
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
        format!("runtime provider `{}` is unavailable", self.safe_id())
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

fn sanitize_provider_id(id: &str) -> String {
    let trimmed = id.trim();
    if is_safe_provider_id(trimmed) && is_safe_provider_message(trimmed) {
        trimmed.to_owned()
    } else {
        "invalid".to_owned()
    }
}

fn is_safe_provider_id(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    value.len() <= 64
        && first.is_ascii_alphanumeric()
        && value.chars().all(|ch| {
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_' | '.')
        })
}

fn is_safe_provider_message(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    !value.contains("://")
        && !value.contains("/Users/")
        && !value.contains("\\Users\\")
        && !value.contains("file://")
        && !value.contains("Bearer ")
        && !lower.contains("authorization")
        && !lower.contains("api_key")
        && !lower.contains("apikey")
        && !lower.contains("token")
        && !lower.contains("secret")
        && !lower.contains("password")
        && !lower.contains("signed_url")
        && !lower.contains("signedurl")
        && !lower.contains("sk-")
}
