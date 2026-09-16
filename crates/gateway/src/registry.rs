use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::{
    ApiConnectorSummary, CostEstimate, ImageProcessingCapabilities, ImageProcessingOutput,
    ImageProcessingRequest, ImageProcessingSubmission, Provider, ProviderCatalog,
    ProviderCatalogSnapshot, ProviderError, ProviderHealth, ProviderRequest, ProviderResult,
    ProviderResultValue, ProviderTaskHandle, RuntimeProvider, RuntimeProviderSummary,
    WorkflowBackendSummary, sanitize_provider_id,
};

#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    providers: BTreeMap<String, RuntimeProvider>,
    default_provider: String,
}

impl ProviderRegistry {
    pub fn from_env() -> Self {
        let runtime_provider = std::env::var("HELIXFLOW_RUNTIME_PROVIDER").ok();
        let enable_mock_provider = std::env::var("HELIXFLOW_ENABLE_MOCK_PROVIDER").ok();
        Self::from_config(
            runtime_provider.as_deref(),
            enable_mock_provider.as_deref(),
            RuntimeProvider::atlas_from_env(),
            RuntimeProvider::fal_from_env(),
        )
    }

    fn from_config(
        runtime_provider: Option<&str>,
        enable_mock_provider: Option<&str>,
        atlas: RuntimeProvider,
        fal: RuntimeProvider,
    ) -> Self {
        let requested = runtime_provider
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(sanitize_provider_id);
        let mut providers = vec![
            RuntimeProvider::unavailable(
                "unconfigured",
                "HELIXFLOW_RUNTIME_PROVIDER is not configured",
            ),
            atlas,
            fal,
        ];
        if requested.as_deref() == Some("mock") {
            providers.push(if mock_provider_enabled(enable_mock_provider) {
                RuntimeProvider::mock()
            } else {
                RuntimeProvider::unavailable(
                    "mock",
                    "mock provider is disabled; explicit dev/test enablement is required",
                )
            });
        }
        // Prefer an already-configured real provider when the env default is unset.
        // That keeps fail-closed (no silent mock) while making `ATLAS_API_KEY=... cargo run`
        // enough to boot — no separate HELIXFLOW_RUNTIME_PROVIDER required.
        let default_provider = resolve_default_provider(requested.as_deref(), &providers);
        Self::new(default_provider, providers)
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

    pub fn image_processing_capabilities(
        &self,
        provider_id: &str,
    ) -> ProviderResultValue<ImageProcessingCapabilities> {
        self.provider_for(provider_id)?
            .image_processing_capabilities()
    }

    pub async fn process_image(
        &self,
        provider_id: &str,
        request: ImageProcessingRequest,
    ) -> ProviderResultValue<ImageProcessingOutput> {
        self.provider_for(provider_id)?.process_image(request).await
    }

    pub async fn submit_image(
        &self,
        provider_id: &str,
        request: ImageProcessingRequest,
    ) -> ProviderResultValue<ImageProcessingSubmission> {
        self.provider_for(provider_id)?.submit_image(request).await
    }
}

/// Resolve the process default provider id.
///
/// Explicit `HELIXFLOW_RUNTIME_PROVIDER` always wins. When unset, pick the first
/// enabled real provider in preference order (`atlas`, then `fal`). With no
/// enabled real provider, stay on synthetic `unconfigured` (fail closed; never
/// auto-enable mock).
fn resolve_default_provider(requested: Option<&str>, providers: &[RuntimeProvider]) -> String {
    if let Some(id) = requested.filter(|value| !value.is_empty()) {
        return id.to_owned();
    }
    for preferred in ["atlas", "fal"] {
        if providers
            .iter()
            .any(|provider| provider.safe_id() == preferred && provider.is_enabled())
        {
            return preferred.to_owned();
        }
    }
    "unconfigured".to_owned()
}

fn mock_provider_enabled(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

#[async_trait]
impl Provider for ProviderRegistry {
    fn id(&self) -> &str {
        "registry"
    }

    fn config_fingerprint(&self, provider_id: &str) -> String {
        self.providers
            .get(provider_id)
            .map(|provider| provider.config_fingerprint(provider_id))
            .unwrap_or_default()
    }

    fn dispatch_origin(&self, provider_id: &str) -> ProviderResultValue<String> {
        self.provider_for(provider_id)?.dispatch_origin(provider_id)
    }

    fn recovery_scope_fingerprint(&self, provider_id: &str) -> String {
        self.providers
            .get(provider_id)
            .map(|provider| provider.recovery_scope_fingerprint(provider_id))
            .unwrap_or_default()
    }

    fn catalog_revision(&self, provider_id: &str) -> String {
        match serde_json::to_string(&self.catalog_snapshot_for_selected(Some(provider_id))) {
            Ok(revision) => revision,
            Err(error) => format!("catalog-serialization-error:{error}"),
        }
    }

    fn recovery_capabilities(&self, provider_id: &str) -> crate::ProviderRecoveryCapabilities {
        self.providers
            .get(provider_id)
            .map(|provider| provider.recovery_capabilities(provider_id))
            .unwrap_or(crate::ProviderRecoveryCapabilities {
                resume: false,
                cancel: false,
            })
    }

    async fn health(&self) -> ProviderHealth {
        ProviderHealth {
            ok: self.providers.values().any(RuntimeProvider::is_enabled),
            message: Some("provider registry ready".to_owned()),
        }
    }

    async fn active_handles(&self, run_id: &str) -> Vec<ProviderTaskHandle> {
        let mut handles = Vec::new();
        for provider in self.providers.values() {
            handles.extend(provider.active_handles(run_id).await);
        }
        handles
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

    async fn dispatch(&self, req: ProviderRequest) -> crate::ProviderDispatchResult {
        let provider = self
            .provider_for(&req.provider)
            .map_err(crate::ProviderDispatchFailure::classify)?;
        provider.dispatch(req).await
    }

    async fn resume(
        &self,
        task: &crate::DurableProviderTask,
        req: &ProviderRequest,
    ) -> ProviderResultValue<crate::ProviderResume> {
        self.provider_for(&task.provider)?.resume(task, req).await
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        self.provider_for(&handle.provider)?.cancel(handle).await
    }

    async fn cancel_durable(&self, task: &crate::DurableProviderTask) -> ProviderResultValue<()> {
        self.provider_for(&task.provider)?
            .cancel_durable(task)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ApiProviderConfig, AtlasProvider, FalProvider, FalProviderConfig};

    fn registry_from_test_config(
        runtime_provider: Option<&str>,
        enable_mock_provider: Option<&str>,
    ) -> ProviderRegistry {
        ProviderRegistry::from_config(
            runtime_provider,
            enable_mock_provider,
            RuntimeProvider::unavailable("atlas", "Atlas provider is not configured"),
            RuntimeProvider::unavailable("fal", "FAL_KEY is not configured"),
        )
    }

    fn enabled_atlas() -> RuntimeProvider {
        RuntimeProvider::Atlas(AtlasProvider::new(ApiProviderConfig::atlas(
            "test-key".to_owned(),
            "https://example.test".to_owned(),
        )))
    }

    fn enabled_fal() -> RuntimeProvider {
        RuntimeProvider::Fal(FalProvider::new(FalProviderConfig::new(
            "test-key".to_owned(),
            "https://example.test".to_owned(),
        )))
    }

    #[test]
    fn production_config_without_provider_fails_closed() {
        let registry = registry_from_test_config(None, None);

        assert_eq!(registry.default_provider(), "unconfigured");
        assert!(!registry.contains_provider("mock"));
        assert!(!registry.provider_enabled("unconfigured"));
        assert_eq!(registry.selected_provider(None), "unconfigured");
    }

    #[test]
    fn unset_env_defaults_to_enabled_atlas() {
        let registry = ProviderRegistry::from_config(
            None,
            None,
            enabled_atlas(),
            RuntimeProvider::unavailable("fal", "FAL_KEY is not configured"),
        );

        assert_eq!(registry.default_provider(), "atlas");
        assert!(registry.provider_enabled("atlas"));
        assert_eq!(registry.selected_provider(None), "atlas");
        assert!(!registry.provider_enabled("unconfigured"));
    }

    #[test]
    fn unset_env_prefers_atlas_when_both_real_providers_are_enabled() {
        let registry = ProviderRegistry::from_config(None, None, enabled_atlas(), enabled_fal());

        assert_eq!(registry.default_provider(), "atlas");
        assert!(registry.provider_enabled("fal"));
    }

    #[test]
    fn unset_env_defaults_to_fal_when_only_fal_is_enabled() {
        let registry = ProviderRegistry::from_config(
            None,
            None,
            RuntimeProvider::unavailable("atlas", "Atlas provider is not configured"),
            enabled_fal(),
        );

        assert_eq!(registry.default_provider(), "fal");
        assert_eq!(registry.selected_provider(None), "fal");
    }

    #[test]
    fn explicit_runtime_provider_overrides_auto_default() {
        let registry =
            ProviderRegistry::from_config(Some("fal"), None, enabled_atlas(), enabled_fal());

        assert_eq!(registry.default_provider(), "fal");
    }

    #[test]
    fn mock_requires_explicit_provider_and_dev_test_switch() {
        for (provider, switch) in [
            (Some("mock"), None),
            (None, Some("true")),
            (Some("mock"), Some("false")),
            (Some("mock"), Some("typo")),
        ] {
            let registry = registry_from_test_config(provider, switch);
            assert!(
                !registry.provider_enabled("mock"),
                "mock must remain unavailable for provider={provider:?}, switch={switch:?}"
            );
        }

        let registry = registry_from_test_config(Some("mock"), Some("true"));
        assert_eq!(registry.default_provider(), "mock");
        assert!(registry.provider_enabled("mock"));
    }
}
