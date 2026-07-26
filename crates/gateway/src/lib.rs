use std::collections::BTreeMap;
use std::fmt;

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

mod atlas;
mod fal;
mod mock_media;
mod registry;
mod runtime_provider;

pub use atlas::{ApiProviderConfig, AtlasProvider};
pub use fal::{FalProvider, FalProviderConfig};
pub use registry::ProviderRegistry;
pub use runtime_provider::{RuntimeProvider, UnavailableProvider};

pub fn module_name() -> &'static str {
    "gateway"
}

pub type ProviderResultValue<T> = Result<T, ProviderError>;

#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    /// Fingerprint of the effective configuration (models, API base, catalog
    /// revision) for the given provider id. Cached artifacts are keyed on
    /// this so config changes invalidate stale entries (HF-021).
    fn config_fingerprint(&self, _provider_id: &str) -> String {
        String::new()
    }
    async fn health(&self) -> ProviderHealth;
    /// Remote task handles currently in flight for the given run, so an
    /// interrupt can cancel remote paid work instead of only dropping the
    /// local future (HF-011).
    async fn active_handles(&self, _run_id: &str) -> Vec<ProviderTaskHandle> {
        Vec::new()
    }
    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog>;
    async fn estimate(&self, req: ProviderRequest) -> ProviderResultValue<CostEstimate>;
    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult>;
    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderHealth {
    pub ok: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderCatalog {
    pub provider: String,
    pub capabilities: BTreeMap<String, ProviderCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCatalogSnapshot {
    pub default_provider: String,
    pub runtime_providers: Vec<RuntimeProviderSummary>,
    pub workflow_backends: Vec<WorkflowBackendSummary>,
    pub api_connectors: Vec<ApiConnectorSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProviderSummary {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub enabled: bool,
    pub status: String,
    pub message: Option<String>,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowBackendSummary {
    pub id: String,
    pub label: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApiConnectorSummary {
    pub id: String,
    pub provider: String,
    pub capability: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderCapability {
    pub artifact_kind: ArtifactKind,
    pub output_name: String,
    pub mime: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderRequest {
    pub provider: String,
    pub capability: String,
    pub node_id: String,
    pub run_id: String,
    pub inputs: BTreeMap<String, ArtifactRef>,
    /// Upstream text inputs materialized by the executor, keyed by input
    /// port. Providers must prefer these over hand-typed params so graph
    /// wiring has real semantics (HF-003).
    #[serde(default)]
    pub input_texts: BTreeMap<String, String>,
    pub params: Value,
    /// Canonical model id resolved at run creation (GH130 T4). Providers
    /// never choose a model themselves.
    #[serde(default)]
    pub resolved_model_id: Option<String>,
    /// Provider-native operation for the resolved binding (e.g. the atlas
    /// model path or the fal queue path). Required for model-bearing
    /// capabilities; absence is an explicit error, never a default.
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactRef {
    pub artifact_id: String,
    pub storage_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderResult {
    pub outputs: BTreeMap<String, ArtifactPayload>,
    pub cost: CostEstimate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArtifactPayload {
    pub kind: ArtifactKind,
    pub mime: String,
    pub storage_uri: String,
    #[serde(default)]
    pub content: ArtifactContent,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u32>,
    pub meta: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArtifactContent {
    InlineBytes {
        bytes: Vec<u8>,
        ext_hint: Option<String>,
    },
    RemoteUrl {
        url: String,
    },
    #[default]
    None,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Text,
    Image,
    Video,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CostEstimate {
    pub amount: f64,
    pub currency: String,
    pub estimated: bool,
    /// True when the provider cannot produce a trustworthy amount. Unknown
    /// costs must never be treated as free (HF-004).
    #[serde(default)]
    pub unknown: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderTaskHandle {
    pub provider: String,
    pub provider_task_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    WrongProvider { expected: String, actual: String },
    UnsupportedCapability(String),
    CancelUnsupported(String),
    Unavailable { provider: String, reason: String },
    InvalidRequest(String),
    InvalidResponse(String),
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

#[derive(Debug, Default, Clone)]
pub struct MockProvider;

impl MockProvider {
    pub fn new() -> Self {
        Self
    }

    pub(crate) fn catalog_value() -> ProviderCatalog {
        ProviderCatalog {
            provider: "mock".to_owned(),
            capabilities: BTreeMap::from([
                (
                    "prompt_writer".to_owned(),
                    ProviderCapability {
                        artifact_kind: ArtifactKind::Text,
                        output_name: "prompt".to_owned(),
                        mime: "text/plain".to_owned(),
                    },
                ),
                (
                    "text_to_image".to_owned(),
                    ProviderCapability {
                        artifact_kind: ArtifactKind::Image,
                        output_name: "image".to_owned(),
                        mime: "image/png".to_owned(),
                    },
                ),
                (
                    "text_to_video".to_owned(),
                    ProviderCapability {
                        artifact_kind: ArtifactKind::Video,
                        output_name: "video".to_owned(),
                        mime: "video/mp4".to_owned(),
                    },
                ),
            ]),
        }
    }

    fn ensure_provider(&self, req: &ProviderRequest) -> ProviderResultValue<()> {
        if req.provider == self.id() {
            return Ok(());
        }

        Err(ProviderError::WrongProvider {
            expected: self.id().to_owned(),
            actual: req.provider.clone(),
        })
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn id(&self) -> &str {
        "mock"
    }

    async fn health(&self) -> ProviderHealth {
        ProviderHealth {
            ok: true,
            message: Some(
                "non-production synthetic mock provider enabled for local testing".to_owned(),
            ),
        }
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog> {
        Ok(Self::catalog_value())
    }

    async fn estimate(&self, req: ProviderRequest) -> ProviderResultValue<CostEstimate> {
        self.ensure_provider(&req)?;
        if !Self::catalog_value()
            .capabilities
            .contains_key(&req.capability)
        {
            return Err(ProviderError::UnsupportedCapability(req.capability));
        }

        Ok(CostEstimate {
            amount: 0.0,
            currency: "USD".to_owned(),
            estimated: true,
            unknown: false,
        })
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        self.ensure_provider(&req)?;

        let catalog = Self::catalog_value();
        let Some(capability) = catalog.capabilities.get(&req.capability) else {
            return Err(ProviderError::UnsupportedCapability(req.capability));
        };

        let artifact = deterministic_artifact(&req, capability)?;

        Ok(ProviderResult {
            outputs: BTreeMap::from([(capability.output_name.clone(), artifact)]),
            cost: CostEstimate {
                amount: 0.0,
                currency: "USD".to_owned(),
                estimated: false,
                unknown: false,
            },
        })
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        Err(ProviderError::CancelUnsupported(handle.provider_task_id))
    }
}

fn deterministic_artifact(
    req: &ProviderRequest,
    capability: &ProviderCapability,
) -> ProviderResultValue<ArtifactPayload> {
    let extension = match capability.artifact_kind {
        ArtifactKind::Text => "txt",
        ArtifactKind::Image => "png",
        ArtifactKind::Video => "mp4",
        ArtifactKind::Json => "json",
    };
    let storage_uri = format!(
        "workspace://outputs/{}/{}/{}.{}",
        req.run_id, req.node_id, req.capability, extension
    );

    let (width, height, duration_ms) = match capability.artifact_kind {
        ArtifactKind::Image => (Some(1), Some(1), None),
        ArtifactKind::Video => (Some(16), Some(16), Some(40)),
        ArtifactKind::Text | ArtifactKind::Json => (None, None, None),
    };

    Ok(ArtifactPayload {
        kind: capability.artifact_kind,
        mime: capability.mime.clone(),
        storage_uri,
        content: mock_content(req, capability)?,
        width,
        height,
        duration_ms,
        meta: json!({
            "provider": req.provider,
            "capability": req.capability,
            "deterministic": true
        }),
    })
}

fn mock_content(
    req: &ProviderRequest,
    capability: &ProviderCapability,
) -> ProviderResultValue<ArtifactContent> {
    Ok(match capability.artifact_kind {
        ArtifactKind::Text => ArtifactContent::InlineBytes {
            bytes: format!(
                "provider=mock\ncapability={}\nnode={}\n",
                req.capability, req.node_id
            )
            .into_bytes(),
            ext_hint: Some("txt".to_owned()),
        },
        ArtifactKind::Image => ArtifactContent::InlineBytes {
            bytes: decode_mock_media(mock_media::PNG_BASE64, "PNG")?,
            ext_hint: Some("png".to_owned()),
        },
        ArtifactKind::Video => ArtifactContent::InlineBytes {
            bytes: decode_mock_media(mock_media::MP4_BASE64, "MP4")?,
            ext_hint: Some("mp4".to_owned()),
        },
        ArtifactKind::Json => ArtifactContent::InlineBytes {
            bytes: serde_json::to_vec(&json!({
                "provider": "mock",
                "capability": req.capability,
            }))
            .map_err(|_| {
                ProviderError::InvalidResponse(
                    "embedded mock JSON fixture could not be serialized".to_owned(),
                )
            })?,
            ext_hint: Some("json".to_owned()),
        },
    })
}

fn decode_mock_media(encoded: &str, kind: &str) -> ProviderResultValue<Vec<u8>> {
    STANDARD.decode(encoded).map_err(|_| {
        ProviderError::InvalidResponse(format!("embedded mock {kind} fixture is invalid"))
    })
}

pub fn sanitize_provider_id(id: &str) -> String {
    let trimmed = id.trim();
    if is_safe_provider_id(trimmed) && is_safe_provider_message(trimmed) {
        trimmed.to_owned()
    } else {
        "invalid".to_owned()
    }
}

pub fn safe_provider_message(value: &str) -> String {
    if is_safe_provider_message(value) {
        value.to_owned()
    } else {
        "provider message was redacted".to_owned()
    }
}

pub fn is_safe_provider_message(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    !value.contains("://")
        && !value.contains("/Users/")
        && !value.contains("\\Users\\")
        && !value.contains("file://")
        && !lower.contains("bearer ")
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

/// Resolve a provider text argument: a wired upstream input on `input_port`
/// wins over the hand-typed `param_key`; missing both is an invalid request.
pub fn wired_or_param_string(
    req: &ProviderRequest,
    input_port: &str,
    param_key: &str,
) -> ProviderResultValue<String> {
    if let Some(text) = req
        .input_texts
        .get(input_port)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        return Ok(text.to_owned());
    }
    req.params
        .get(param_key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            ProviderError::InvalidRequest(format!(
                "missing wired input `{input_port}` and string param `{param_key}`"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(capability: &str) -> ProviderRequest {
        ProviderRequest {
            provider: "mock".to_owned(),
            capability: capability.to_owned(),
            node_id: "n4".to_owned(),
            run_id: "run_123".to_owned(),
            inputs: BTreeMap::new(),
            input_texts: BTreeMap::new(),
            params: json!({
                "prompt": "A clean product shot",
                "duration_sec": 5,
                "aspect_ratio": "9:16"
            }),
            resolved_model_id: None,
            operation_id: None,
        }
    }

    #[test]
    fn reports_module_name() {
        assert_eq!(module_name(), "gateway");
    }

    #[test]
    fn wired_input_wins_over_param() {
        let mut req = request("prompt_writer");
        req.params = json!({ "prompt": "typed prompt" });
        req.input_texts
            .insert("text".to_owned(), "wired text".to_owned());
        let value = wired_or_param_string(&req, "text", "prompt").expect("resolve");
        assert_eq!(value, "wired text");
    }

    #[test]
    fn param_is_fallback_when_no_wired_input() {
        let mut req = request("prompt_writer");
        req.params = json!({ "prompt": "typed prompt" });
        let value = wired_or_param_string(&req, "text", "prompt").expect("resolve");
        assert_eq!(value, "typed prompt");
    }

    #[test]
    fn missing_wired_input_and_param_is_invalid_request() {
        let mut req = request("prompt_writer");
        req.params = json!({});
        let err = wired_or_param_string(&req, "text", "prompt").expect_err("must fail");
        assert!(matches!(err, ProviderError::InvalidRequest(_)));
    }

    #[test]
    fn serializes_cost_estimate_boundary() {
        let estimate = CostEstimate {
            amount: 0.42,
            currency: "USD".to_string(),
            estimated: true,
            unknown: false,
        };

        let encoded = serde_json::to_value(&estimate).expect("serialize estimate");

        assert_eq!(encoded["amount"], 0.42);
        assert_eq!(encoded["currency"], "USD");
        assert_eq!(encoded["estimated"], true);
    }

    #[tokio::test]
    async fn mock_provider_returns_deterministic_video_artifact() {
        let provider = MockProvider::new();

        let first = provider
            .invoke(request("text_to_video"))
            .await
            .expect("first invoke");
        let second = provider
            .invoke(request("text_to_video"))
            .await
            .expect("second invoke");
        let artifact = first.outputs.get("video").expect("video output");

        assert_eq!(first, second);
        assert_eq!(artifact.kind, ArtifactKind::Video);
        assert_eq!(artifact.mime, "video/mp4");
        assert_eq!(
            artifact.storage_uri,
            "workspace://outputs/run_123/n4/text_to_video.mp4"
        );
        assert_eq!(artifact.width, Some(16));
        assert_eq!(artifact.height, Some(16));
        assert_eq!(artifact.duration_ms, Some(40));
    }

    #[tokio::test]
    async fn mock_provider_rejects_unknown_capability() {
        let provider = MockProvider::new();

        let err = provider
            .invoke(request("missing"))
            .await
            .expect_err("missing capability should fail");

        assert_eq!(
            err,
            ProviderError::UnsupportedCapability("missing".to_owned())
        );
    }

    #[tokio::test]
    async fn mock_provider_rejects_legacy_capability_id_from_pre_rename_runs() {
        // Pre-GH145 plans carry `image_generate`; retries fail explicitly
        // instead of silently executing under `text_to_image`.
        let provider = MockProvider::new();

        let err = provider
            .invoke(request("image_generate"))
            .await
            .expect_err("legacy capability id should fail");

        assert_eq!(
            err,
            ProviderError::UnsupportedCapability("image_generate".to_owned())
        );
    }

    #[tokio::test]
    async fn runtime_provider_delegates_to_mock_provider() {
        let provider = RuntimeProvider::mock();

        let result = provider
            .invoke(request("prompt_writer"))
            .await
            .expect("mock runtime provider invoke");

        assert_eq!(provider.id(), "mock");
        assert!(result.outputs.contains_key("prompt"));
    }

    #[test]
    fn mock_runtime_provider_exports_safe_catalog_snapshot() {
        let provider = RuntimeProvider::mock();

        let snapshot = provider.catalog_snapshot();

        assert_eq!(snapshot.default_provider, "mock");
        assert_eq!(snapshot.runtime_providers[0].id, "mock");
        assert_eq!(snapshot.runtime_providers[0].label, "Mock (local test)");
        assert_eq!(snapshot.runtime_providers[0].kind, "local_test");
        assert!(snapshot.runtime_providers[0].enabled);
        assert_eq!(snapshot.runtime_providers[0].status, "healthy");
        assert!(
            snapshot.runtime_providers[0]
                .message
                .as_deref()
                .is_some_and(|message| message.contains("non-production synthetic"))
        );
        assert!(
            snapshot.runtime_providers[0]
                .capabilities
                .contains(&"text_to_video".to_owned())
        );
        assert_eq!(snapshot.workflow_backends[0].id, "helixflow_graph");
        assert!(
            snapshot
                .api_connectors
                .iter()
                .any(|connector| connector.id == "mock.text_to_video")
        );

        let encoded = match serde_json::to_string(&snapshot) {
            Ok(value) => value,
            Err(err) => panic!("serialize provider snapshot: {err}"),
        };
        assert!(!encoded.contains("API_KEY"));
        assert!(!encoded.contains("Authorization"));
    }

    #[tokio::test]
    async fn unavailable_runtime_provider_rejects_invocation() {
        let provider = RuntimeProvider::unavailable("openai", "missing connector config");

        let health = provider.health().await;
        let err = provider
            .invoke(ProviderRequest {
                provider: "openai".to_owned(),
                ..request("prompt_writer")
            })
            .await
            .expect_err("unavailable provider should reject invoke");

        assert_eq!(provider.id(), "openai");
        assert!(!health.ok);
        assert_eq!(
            err,
            ProviderError::Unavailable {
                provider: "openai".to_owned(),
                reason: "missing connector config".to_owned(),
            }
        );
    }

    #[test]
    fn unavailable_runtime_provider_exports_unavailable_snapshot_without_mock_fallback() {
        let provider = RuntimeProvider::unavailable("openai", "missing connector config");

        let snapshot = provider.catalog_snapshot();

        assert_eq!(snapshot.default_provider, "openai");
        assert_eq!(snapshot.runtime_providers[0].id, "openai");
        assert_eq!(snapshot.runtime_providers[0].kind, "unavailable");
        assert!(!snapshot.runtime_providers[0].enabled);
        assert_eq!(snapshot.runtime_providers[0].status, "unavailable");
        assert_eq!(
            snapshot.runtime_providers[0].message.as_deref(),
            Some("missing connector config")
        );
        assert!(snapshot.runtime_providers[0].capabilities.is_empty());
        assert!(snapshot.workflow_backends.is_empty());
        assert!(snapshot.api_connectors.is_empty());
    }

    #[test]
    fn unavailable_runtime_provider_snapshot_redacts_unsafe_env_values() {
        let provider = RuntimeProvider::unavailable(
            "sk-secret-token",
            "runtime provider `sk-secret-token` from https://example.test/key is not configured",
        );

        let snapshot = provider.catalog_snapshot();
        let encoded = match serde_json::to_string(&snapshot) {
            Ok(value) => value,
            Err(err) => panic!("serialize provider snapshot: {err}"),
        };

        assert_eq!(snapshot.default_provider, "invalid");
        assert_eq!(snapshot.runtime_providers[0].id, "invalid");
        assert_eq!(
            snapshot.runtime_providers[0].message.as_deref(),
            Some("runtime provider `invalid` is unavailable")
        );
        assert!(!encoded.contains("sk-secret-token"));
        assert!(!encoded.contains("https://example.test"));
    }

    #[test]
    fn unavailable_runtime_provider_snapshot_redacts_bearer_markers_case_insensitively() {
        for marker in ["bearer eyJhbGciOiJIUzI1NiJ9", "bEaReR abc123"] {
            let provider = RuntimeProvider::unavailable(
                "openai",
                format!("runtime provider `openai` returned {marker}"),
            );

            let snapshot = provider.catalog_snapshot();
            let encoded = match serde_json::to_string(&snapshot) {
                Ok(value) => value,
                Err(err) => panic!("serialize provider snapshot: {err}"),
            };
            let lower = encoded.to_ascii_lowercase();

            assert_eq!(snapshot.default_provider, "openai");
            assert_eq!(
                snapshot.runtime_providers[0].message.as_deref(),
                Some("runtime provider `openai` is unavailable")
            );
            assert!(!lower.contains("bearer"));
            assert!(!encoded.contains("eyJ"));
            assert!(!encoded.contains("abc123"));
        }
    }
}
