use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::{
    ApiConnectorSummary, CostEstimate, Provider, ProviderCatalog, ProviderCatalogSnapshot,
    ProviderError, ProviderHealth, ProviderRequest, ProviderResult, ProviderResultValue,
    ProviderTaskHandle, RuntimeProvider, RuntimeProviderSummary, WorkflowBackendSummary,
    sanitize_provider_id,
};

#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    providers: BTreeMap<String, RuntimeProvider>,
    default_provider: String,
}

impl ProviderRegistry {
    pub fn from_env() -> Self {
        let default_provider = std::env::var("HELIXFLOW_RUNTIME_PROVIDER")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "mock".to_owned());
        Self::new(
            default_provider,
            vec![
                RuntimeProvider::mock(),
                RuntimeProvider::atlas_from_env(),
                RuntimeProvider::fal_from_env(),
            ],
        )
    }

    pub fn new(default_provider: impl Into<String>, providers: Vec<RuntimeProvider>) -> Self {
        let requested_default = sanitize_provider_id(&default_provider.into());
        let mut provider_map = providers
            .into_iter()
            .map(|provider| (provider.safe_id(), provider))
            .collect::<BTreeMap<_, _>>();
        if !provider_map.contains_key(&requested_default) {
            provider_map.insert(
                requested_default.clone(),
                RuntimeProvider::unavailable(
                    requested_default.clone(),
                    "provider is not registered in this build",
                ),
            );
        }
        Self {
            providers: provider_map,
            default_provider: requested_default,
        }
    }

    pub fn default_provider(&self) -> &str {
        &self.default_provider
    }

    pub fn contains_provider(&self, provider_id: &str) -> bool {
        self.providers.contains_key(provider_id)
    }

    pub fn provider_enabled(&self, provider_id: &str) -> bool {
        self.providers
            .get(provider_id)
            .is_some_and(RuntimeProvider::is_enabled)
    }

    pub fn selected_provider(&self, persisted: Option<&str>) -> String {
        persisted
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(sanitize_provider_id)
            .unwrap_or_else(|| self.default_provider.clone())
    }

    pub fn catalog_snapshot(&self) -> ProviderCatalogSnapshot {
        self.catalog_snapshot_for_selected(None)
    }

    pub fn catalog_snapshot_for_selected(
        &self,
        selected_provider: Option<&str>,
    ) -> ProviderCatalogSnapshot {
        let mut runtime_providers = Vec::new();
        let mut api_connectors = Vec::new();
        let mut any_enabled = false;

        for (provider_id, provider) in &self.providers {
            let snapshot = provider.catalog_snapshot();
            if let Some(summary) = snapshot.runtime_providers.into_iter().next() {
                any_enabled |= summary.enabled;
                api_connectors.extend(summary.capabilities.iter().map(|capability| {
                    ApiConnectorSummary {
                        id: format!("{}.{}", provider_id, capability),
                        provider: provider_id.clone(),
                        capability: capability.clone(),
                        status: summary.status.clone(),
                    }
                }));
                runtime_providers.push(summary);
            }
        }
        if let Some(selected_provider) = selected_provider.map(sanitize_provider_id)
            && !runtime_providers
                .iter()
                .any(|provider| provider.id == selected_provider)
        {
            runtime_providers.push(RuntimeProviderSummary {
                id: selected_provider.clone(),
                label: selected_provider,
                kind: "unavailable".to_owned(),
                enabled: false,
                status: "unavailable".to_owned(),
                message: Some("provider is not registered in this build".to_owned()),
                capabilities: Vec::new(),
            });
        }

        ProviderCatalogSnapshot {
            default_provider: self.default_provider.clone(),
            runtime_providers,
            workflow_backends: any_enabled
                .then(|| WorkflowBackendSummary {
                    id: "helixflow_graph".to_owned(),
                    label: "Helixflow Graph".to_owned(),
                    status: "healthy".to_owned(),
                })
                .into_iter()
                .collect(),
            api_connectors,
        }
    }

    fn provider_for(&self, provider_id: &str) -> ProviderResultValue<&RuntimeProvider> {
        self.providers
            .get(provider_id)
            .ok_or_else(|| ProviderError::Unavailable {
                provider: sanitize_provider_id(provider_id),
                reason: "provider is not registered in this build".to_owned(),
            })
    }
}

#[async_trait]
impl Provider for ProviderRegistry {
    fn id(&self) -> &str {
        "registry"
    }

    async fn health(&self) -> ProviderHealth {
        ProviderHealth {
            ok: self.providers.values().any(RuntimeProvider::is_enabled),
            message: Some("provider registry ready".to_owned()),
        }
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog> {
        self.provider_for(&self.default_provider)?.catalog().await
    }

    async fn estimate(&self, req: ProviderRequest) -> ProviderResultValue<CostEstimate> {
        self.provider_for(&req.provider)?.estimate(req).await
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        self.provider_for(&req.provider)?.invoke(req).await
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        self.provider_for(&handle.provider)?.cancel(handle).await
    }
}
