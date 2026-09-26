use std::fmt;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    ApiConnectorSummary, AtlasProvider, CostEstimate, FalProvider, ImageProcessingCapabilities,
    ImageProcessingOutput, ImageProcessingRequest, MockProvider, ProviderCatalog,
    ProviderCatalogSnapshot, ProviderHealth, ProviderRequest, ProviderResult,
    RuntimeProviderSummary, WorkflowBackendSummary, is_safe_provider_message,
    safe_provider_message, sanitize_provider_id,
};

pub type ProviderResultValue<T> = Result<T, ProviderError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderTaskHandle {
    pub provider: String,
    pub provider_task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DurableProviderTask {
    pub provider: String,
    pub provider_task_id: String,
    pub dispatch_origin: String,
    pub recovery_scope_fingerprint: String,
    pub status_url: Option<String>,
    pub result_url: Option<String>,
}

impl DurableProviderTask {
    pub fn legacy_handle(&self) -> ProviderTaskHandle {
        ProviderTaskHandle {
            provider: self.provider.clone(),
            provider_task_id: self.provider_task_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderDispatch {
    Completed(ProviderResult),
    Accepted(DurableProviderTask),
}

pub type ProviderDispatchResult = Result<ProviderDispatch, ProviderDispatchFailure>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderDispatchFailureKind {
    NotSubmitted,
    Rejected,
    OutcomeUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDispatchFailure {
    pub kind: ProviderDispatchFailureKind,
    pub error: ProviderError,
}

impl ProviderDispatchFailure {
    pub fn classify(error: ProviderError) -> Self {
        let kind = match error {
            ProviderError::RequestRejected(_) => ProviderDispatchFailureKind::Rejected,
            ProviderError::InvalidResponse(_) | ProviderError::RequestFailed(_) => {
                ProviderDispatchFailureKind::OutcomeUnknown
            }
            _ => ProviderDispatchFailureKind::NotSubmitted,
        };
        Self { kind, error }
    }
}

impl fmt::Display for ProviderDispatchFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for ProviderDispatchFailure {}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderResume {
    Pending {
        retry_after_ms: u64,
    },
    Completed(ProviderResult),
    Failed {
        reason_code: String,
        cost: CostEstimate,
    },
}

impl ProviderResume {
    pub fn failed(reason_code: &str) -> Self {
        Self::Failed {
            reason_code: reason_code.to_owned(),
            cost: CostEstimate {
                amount: 0.0,
                currency: "USD".to_owned(),
                estimated: true,
                unknown: true,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderRecoveryCapabilities {
    pub resume: bool,
    pub cancel: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    WrongProvider { expected: String, actual: String },
    UnsupportedCapability(String),
    CancelUnsupported(String),
    RecoveryUnsupported(String),
    Unavailable { provider: String, reason: String },
    InvalidRequest(String),
    InvalidResponse(String),
    RequestRejected(String),
    RequestFailed(String),
    ModelUnresolved { capability: String },
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongProvider { expected, actual } => {
                write!(f, "wrong provider: expected `{expected}`, got `{actual}`")
            }
            Self::UnsupportedCapability(capability) => {
                write!(f, "unsupported provider capability: {capability}")
            }
            Self::CancelUnsupported(provider_task_id) => {
                write!(
                    f,
                    "provider does not support cancelling remote task: {provider_task_id}"
                )
            }
            Self::RecoveryUnsupported(provider) => {
                write!(f, "provider `{provider}` does not support task recovery")
            }
            Self::Unavailable { provider, reason } => {
                write!(f, "runtime provider `{provider}` is unavailable: {reason}")
            }
            Self::ModelUnresolved { capability } => {
                write!(
                    f,
                    "no resolved model binding for capability `{capability}`; run preflight must resolve implementations before invoke"
                )
            }
            Self::InvalidRequest(message) => {
                write!(
                    f,
                    "invalid provider request: {}",
                    safe_provider_message(message)
                )
            }
            Self::InvalidResponse(message) => {
                write!(
                    f,
                    "invalid provider response: {}",
                    safe_provider_message(message)
                )
            }
            Self::RequestRejected(message) => {
                write!(
                    f,
                    "provider rejected request: {}",
                    safe_provider_message(message)
                )
            }
            Self::RequestFailed(message) => {
                write!(
                    f,
                    "provider request failed: {}",
                    safe_provider_message(message)
                )
            }
        }
    }
}

impl std::error::Error for ProviderError {}

#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;

    fn config_fingerprint(&self, _provider_id: &str) -> String {
        String::new()
    }

    fn dispatch_origin(&self, _provider_id: &str) -> ProviderResultValue<String> {
        Ok(format!("provider://{}", sanitize_provider_id(self.id())))
    }

    fn recovery_scope_fingerprint(&self, provider_id: &str) -> String {
        self.config_fingerprint(provider_id)
    }

    fn catalog_revision(&self, provider_id: &str) -> String {
        self.config_fingerprint(provider_id)
    }

    fn recovery_capabilities(&self, _provider_id: &str) -> ProviderRecoveryCapabilities {
        ProviderRecoveryCapabilities {
            resume: false,
            cancel: false,
        }
    }

    async fn health(&self) -> ProviderHealth;

    async fn active_handles(&self, _run_id: &str) -> Vec<ProviderTaskHandle> {
        Vec::new()
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog>;
    async fn estimate(&self, req: ProviderRequest) -> ProviderResultValue<CostEstimate>;
    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult>;

    async fn dispatch(&self, req: ProviderRequest) -> ProviderDispatchResult {
        self.invoke(req)
            .await
            .map(ProviderDispatch::Completed)
            .map_err(ProviderDispatchFailure::classify)
    }

    async fn resume(
        &self,
        task: &DurableProviderTask,
        _req: &ProviderRequest,
    ) -> ProviderResultValue<ProviderResume> {
        Err(ProviderError::RecoveryUnsupported(task.provider.clone()))
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()>;

    async fn cancel_durable(&self, task: &DurableProviderTask) -> ProviderResultValue<()> {
        self.cancel(task.legacy_handle()).await
    }
}

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

    pub fn image_processing_capabilities(
        &self,
    ) -> ProviderResultValue<ImageProcessingCapabilities> {
        match self {
            Self::Atlas(provider) => Ok(provider.image_processing_capabilities()),
            Self::Unavailable(provider) => Err(ProviderError::Unavailable {
                provider: provider.safe_id(),
                reason: provider.safe_reason(),
            }),
            Self::Mock(_) | Self::Fal(_) => Err(ProviderError::UnsupportedCapability(
                "image_processing".to_owned(),
            )),
        }
    }

    pub async fn submit_image(
        &self,
        request: ImageProcessingRequest,
    ) -> ProviderResultValue<crate::ImageProcessingSubmission> {
        match self {
            Self::Atlas(provider) => provider.submit_image(request).await,
            Self::Unavailable(provider) => Err(ProviderError::Unavailable {
                provider: provider.safe_id(),
                reason: provider.safe_reason(),
            }),
            Self::Mock(_) | Self::Fal(_) => Err(ProviderError::UnsupportedCapability(
                "image_processing".to_owned(),
            )),
        }
    }

    pub async fn process_image(
        &self,
        request: ImageProcessingRequest,
    ) -> ProviderResultValue<ImageProcessingOutput> {
        self.submit_image(request).await?.complete().await
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

    fn config_fingerprint(&self, provider_id: &str) -> String {
        match self {
            Self::Mock(provider) => provider.config_fingerprint(provider_id),
            Self::Atlas(provider) => provider.config_fingerprint(provider_id),
            Self::Fal(provider) => provider.config_fingerprint(provider_id),
            Self::Unavailable(provider) => provider.config_fingerprint(provider_id),
        }
    }

    fn dispatch_origin(&self, provider_id: &str) -> ProviderResultValue<String> {
        match self {
            Self::Mock(provider) => provider.dispatch_origin(provider_id),
            Self::Atlas(provider) => provider.dispatch_origin(provider_id),
            Self::Fal(provider) => provider.dispatch_origin(provider_id),
            Self::Unavailable(provider) => provider.dispatch_origin(provider_id),
        }
    }

    fn recovery_scope_fingerprint(&self, provider_id: &str) -> String {
        match self {
            Self::Mock(provider) => provider.recovery_scope_fingerprint(provider_id),
            Self::Atlas(provider) => provider.recovery_scope_fingerprint(provider_id),
            Self::Fal(provider) => provider.recovery_scope_fingerprint(provider_id),
            Self::Unavailable(provider) => provider.recovery_scope_fingerprint(provider_id),
        }
    }

    fn recovery_capabilities(&self, provider_id: &str) -> ProviderRecoveryCapabilities {
        match self {
            Self::Mock(provider) => provider.recovery_capabilities(provider_id),
            Self::Atlas(provider) => provider.recovery_capabilities(provider_id),
            Self::Fal(provider) => provider.recovery_capabilities(provider_id),
            Self::Unavailable(provider) => provider.recovery_capabilities(provider_id),
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

    async fn active_handles(&self, run_id: &str) -> Vec<ProviderTaskHandle> {
        match self {
            Self::Mock(provider) => provider.active_handles(run_id).await,
            Self::Atlas(provider) => provider.active_handles(run_id).await,
            Self::Fal(provider) => provider.active_handles(run_id).await,
            Self::Unavailable(provider) => provider.active_handles(run_id).await,
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

    async fn dispatch(&self, req: ProviderRequest) -> ProviderDispatchResult {
        match self {
            Self::Mock(provider) => provider.dispatch(req).await,
            Self::Atlas(provider) => provider.dispatch(req).await,
            Self::Fal(provider) => provider.dispatch(req).await,
            Self::Unavailable(provider) => provider.dispatch(req).await,
        }
    }

    async fn resume(
        &self,
        task: &DurableProviderTask,
        req: &ProviderRequest,
    ) -> ProviderResultValue<ProviderResume> {
        match self {
            Self::Mock(provider) => provider.resume(task, req).await,
            Self::Atlas(provider) => provider.resume(task, req).await,
            Self::Fal(provider) => provider.resume(task, req).await,
            Self::Unavailable(provider) => provider.resume(task, req).await,
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

    async fn cancel_durable(&self, task: &DurableProviderTask) -> ProviderResultValue<()> {
        match self {
            Self::Mock(provider) => provider.cancel_durable(task).await,
            Self::Atlas(provider) => provider.cancel_durable(task).await,
            Self::Fal(provider) => provider.cancel_durable(task).await,
            Self::Unavailable(provider) => provider.cancel_durable(task).await,
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
