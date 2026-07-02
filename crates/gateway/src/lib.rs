use std::collections::BTreeMap;
use std::fmt;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub fn module_name() -> &'static str {
    "gateway"
}

pub type ProviderResultValue<T> = Result<T, ProviderError>;

#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    async fn health(&self) -> ProviderHealth;
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
    pub params: Value,
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
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u32>,
    pub meta: Value,
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
    InvalidRequest(String),
    ProviderUnavailable(String),
    RequestFailed(String),
    InvalidResponse(String),
    CancelUnsupported(String),
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
            Self::InvalidRequest(message) => write!(f, "invalid provider request: {message}"),
            Self::ProviderUnavailable(message) => write!(f, "provider unavailable: {message}"),
            Self::RequestFailed(message) => write!(f, "provider request failed: {message}"),
            Self::InvalidResponse(message) => write!(f, "invalid provider response: {message}"),
            Self::CancelUnsupported(provider_task_id) => {
                write!(f, "cancel is unsupported for mock task: {provider_task_id}")
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

    fn catalog_value() -> ProviderCatalog {
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
                    "image_generate".to_owned(),
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
    fn id(&self) -> &'static str {
        "mock"
    }

    async fn health(&self) -> ProviderHealth {
        ProviderHealth {
            ok: true,
            message: Some("mock provider ready".to_owned()),
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
        })
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        self.ensure_provider(&req)?;

        let catalog = Self::catalog_value();
        let Some(capability) = catalog.capabilities.get(&req.capability) else {
            return Err(ProviderError::UnsupportedCapability(req.capability));
        };

        let artifact = deterministic_artifact(&req, capability);

        Ok(ProviderResult {
            outputs: BTreeMap::from([(capability.output_name.clone(), artifact)]),
            cost: CostEstimate {
                amount: 0.0,
                currency: "USD".to_owned(),
                estimated: false,
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
) -> ArtifactPayload {
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
        ArtifactKind::Image => (Some(1024), Some(1024), None),
        ArtifactKind::Video => (Some(1080), Some(1920), Some(duration_ms(&req.params))),
        ArtifactKind::Text | ArtifactKind::Json => (None, None, None),
    };

    ArtifactPayload {
        kind: capability.artifact_kind,
        mime: capability.mime.clone(),
        storage_uri,
        width,
        height,
        duration_ms,
        meta: json!({
            "provider": req.provider,
            "capability": req.capability,
            "deterministic": true
        }),
    }
}

fn duration_ms(params: &Value) -> u32 {
    params
        .get("duration_sec")
        .and_then(Value::as_u64)
        .map(|seconds| seconds.saturating_mul(1000).min(u32::MAX as u64) as u32)
        .unwrap_or(1000)
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
            params: json!({
                "prompt": "A clean product shot",
                "duration_sec": 5,
                "aspect_ratio": "9:16"
            }),
        }
    }

    #[test]
    fn reports_module_name() {
        assert_eq!(module_name(), "gateway");
    }

    #[test]
    fn serializes_cost_estimate_boundary() {
        let estimate = CostEstimate {
            amount: 0.42,
            currency: "USD".to_string(),
            estimated: true,
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
        assert_eq!(artifact.duration_ms, Some(5000));
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
}
