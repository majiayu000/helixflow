use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use helixflow_gateway::{
    ArtifactKind, ArtifactPayload, CostEstimate, Provider, ProviderCapability, ProviderCatalog,
    ProviderError, ProviderHealth, ProviderRequest, ProviderResult, ProviderResultValue,
    ProviderTaskHandle,
};
use serde_json::{Value, json};

const DEFAULT_ATLAS_API_BASE: &str = "https://api.atlascloud.ai/v1";

#[derive(Debug, Clone)]
pub enum ConfiguredProvider {
    Atlas(AtlasProvider),
    None(NoConfiguredProvider),
}

impl ConfiguredProvider {
    pub fn from_env() -> Self {
        AtlasProvider::from_env()
            .map(Self::Atlas)
            .unwrap_or_else(|| Self::None(NoConfiguredProvider))
    }

    pub fn status_label(&self) -> &'static str {
        match self {
            Self::Atlas(_) => "Atlas API",
            Self::None(_) => "provider unconfigured",
        }
    }

    pub fn endpoint(&self) -> Option<&str> {
        match self {
            Self::Atlas(provider) => Some(provider.api_base()),
            Self::None(_) => None,
        }
    }
}

#[async_trait]
impl Provider for ConfiguredProvider {
    fn id(&self) -> &'static str {
        match self {
            Self::Atlas(provider) => provider.id(),
            Self::None(provider) => provider.id(),
        }
    }

    async fn health(&self) -> ProviderHealth {
        match self {
            Self::Atlas(provider) => provider.health().await,
            Self::None(provider) => provider.health().await,
        }
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog> {
        match self {
            Self::Atlas(provider) => provider.catalog().await,
            Self::None(provider) => provider.catalog().await,
        }
    }

    async fn estimate(&self, req: ProviderRequest) -> ProviderResultValue<CostEstimate> {
        match self {
            Self::Atlas(provider) => provider.estimate(req).await,
            Self::None(provider) => provider.estimate(req).await,
        }
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        match self {
            Self::Atlas(provider) => provider.invoke(req).await,
            Self::None(provider) => provider.invoke(req).await,
        }
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        match self {
            Self::Atlas(provider) => provider.cancel(handle).await,
            Self::None(provider) => provider.cancel(handle).await,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NoConfiguredProvider;

#[async_trait]
impl Provider for NoConfiguredProvider {
    fn id(&self) -> &'static str {
        "none"
    }

    async fn health(&self) -> ProviderHealth {
        ProviderHealth {
            ok: false,
            message: Some("set ATLAS_API_KEY to enable the Atlas API provider".to_owned()),
        }
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog> {
        Err(ProviderError::ProviderUnavailable(
            "set ATLAS_API_KEY to enable the Atlas API provider".to_owned(),
        ))
    }

    async fn estimate(&self, req: ProviderRequest) -> ProviderResultValue<CostEstimate> {
        Err(ProviderError::ProviderUnavailable(format!(
            "provider `{}` is not configured; set ATLAS_API_KEY",
            req.provider
        )))
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        Err(ProviderError::ProviderUnavailable(format!(
            "provider `{}` is not configured; set ATLAS_API_KEY",
            req.provider
        )))
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        Err(ProviderError::CancelUnsupported(handle.provider_task_id))
    }
}

#[derive(Debug, Clone)]
pub struct ApiProviderConfig {
    pub provider_id: &'static str,
    pub api_key: String,
    pub api_base: String,
    pub account_id: Option<String>,
    pub extra_header: Option<(String, String)>,
    pub poll_interval: Duration,
    pub poll_timeout: Duration,
}

impl ApiProviderConfig {
    fn atlas(api_key: String, api_base: String) -> Self {
        Self {
            provider_id: "atlas",
            api_key,
            api_base: normalize_api_base(&api_base),
            account_id: atlas_account_id(),
            extra_header: atlas_extra_header(),
            poll_interval: env_duration_ms("HELIXFLOW_ATLAS_POLL_INTERVAL_MS", 5_000),
            poll_timeout: env_duration_secs("HELIXFLOW_ATLAS_POLL_TIMEOUT_SECS", 600),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AtlasProvider {
    config: ApiProviderConfig,
    client: reqwest::Client,
}

impl AtlasProvider {
    pub fn from_env() -> Option<Self> {
        let api_base =
            std::env::var("ATLAS_API_BASE").unwrap_or_else(|_| DEFAULT_ATLAS_API_BASE.to_owned());
        let api_key = atlas_api_key(&api_base)?;
        Some(Self::new(ApiProviderConfig::atlas(api_key, api_base)))
    }

    pub fn new(config: ApiProviderConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub fn api_base(&self) -> &str {
        &self.config.api_base
    }

    fn api_root(&self) -> &str {
        self.config
            .api_base
            .strip_suffix("/v1")
            .unwrap_or(&self.config.api_base)
    }

    fn llm_api_base(&self) -> String {
        format!("{}/v1", self.api_root())
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

    fn catalog_value() -> ProviderCatalog {
        ProviderCatalog {
            provider: "atlas".to_owned(),
            capabilities: BTreeMap::from([
                (
                    "chat_completion".to_owned(),
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
                    "image_edit".to_owned(),
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
                (
                    "image_to_video".to_owned(),
                    ProviderCapability {
                        artifact_kind: ArtifactKind::Video,
                        output_name: "video".to_owned(),
                        mime: "video/mp4".to_owned(),
                    },
                ),
            ]),
        }
    }

    async fn post_json(&self, url: String, body: Value) -> ProviderResultValue<Value> {
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.config.api_key)
            .headers(self.extra_headers())
            .json(&body)
            .send()
            .await
            .map_err(|err| ProviderError::RequestFailed(err.to_string()))?;
        response_json(response).await
    }

    async fn get_json(&self, url: String) -> ProviderResultValue<Value> {
        let response = self
            .client
            .get(url)
            .bearer_auth(&self.config.api_key)
            .headers(self.extra_headers())
            .send()
            .await
            .map_err(|err| ProviderError::RequestFailed(err.to_string()))?;
        response_json(response).await
    }

    fn extra_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Some(account_id) = &self.config.account_id {
            if let Ok(value) = reqwest::header::HeaderValue::from_str(account_id) {
                headers.insert("x-account-id", value);
            }
        }
        if let Some((name, value)) = &self.config.extra_header {
            let Ok(name) = reqwest::header::HeaderName::from_bytes(name.as_bytes()) else {
                return headers;
            };
            let Ok(value) = reqwest::header::HeaderValue::from_str(value) else {
                return headers;
            };
            headers.insert(name, value);
        }
        headers
    }

    async fn invoke_chat(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        let prompt = required_string(&req.params, "prompt")?;
        let model = optional_string(&req.params, "model")
            .unwrap_or_else(|| "deepseek-ai/DeepSeek-V3-0324".to_owned());
        let max_tokens = req
            .params
            .get("max_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(4096);
        let mut messages = Vec::new();
        if let Some(system) = optional_string(&req.params, "system") {
            messages.push(json!({ "role": "system", "content": system }));
        }
        messages.push(json!({ "role": "user", "content": prompt }));
        let response = self
            .post_json(
                format!("{}/chat/completions", self.llm_api_base()),
                json!({
                    "model": model,
                    "messages": messages,
                    "max_tokens": max_tokens,
                    "stream": false
                }),
            )
            .await?;
        let content = response
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProviderError::InvalidResponse(
                    "chat response missing choices[0].message.content".to_owned(),
                )
            })?;

        Ok(single_output_result(
            "prompt",
            ArtifactPayload {
                kind: ArtifactKind::Text,
                mime: "text/plain".to_owned(),
                storage_uri: format!(
                    "inline://atlas/{}/{}/chat_completion.txt",
                    req.run_id, req.node_id
                ),
                width: None,
                height: None,
                duration_ms: None,
                meta: json!({
                    "provider": "atlas",
                    "model": model,
                    "text": content
                }),
            },
        ))
    }

    async fn invoke_image(
        &self,
        req: ProviderRequest,
        edit: bool,
    ) -> ProviderResultValue<ProviderResult> {
        let prompt = required_string(&req.params, "prompt")?;
        let model = optional_string(&req.params, "model").unwrap_or_else(|| {
            if edit {
                "google/nano-banana-2/edit-developer".to_owned()
            } else {
                "google/nano-banana-2/text-to-image".to_owned()
            }
        });
        let mut body = json!({
            "model": model,
            "prompt": prompt,
            "enable_sync_mode": true,
            "output_format": optional_string(&req.params, "output_format").unwrap_or_else(|| "png".to_owned()),
            "num_images": req.params.get("num_images").and_then(Value::as_u64).unwrap_or(1)
        });
        if edit {
            body["image"] = Value::String(required_image_source(&req)?);
        } else {
            body["aspect_ratio"] = Value::String(
                optional_string(&req.params, "aspect_ratio").unwrap_or_else(|| "1:1".to_owned()),
            );
        }
        if let Some(mask) = optional_string(&req.params, "mask") {
            body["mask"] = Value::String(mask);
        }

        let response = self
            .post_json(
                format!("{}/api/v1/model/generateImage", self.api_root()),
                body,
            )
            .await?;
        let output = first_output(&response)?;
        Ok(single_output_result(
            "image",
            ArtifactPayload {
                kind: ArtifactKind::Image,
                mime: "image/png".to_owned(),
                storage_uri: output,
                width: None,
                height: None,
                duration_ms: None,
                meta: json!({
                    "provider": "atlas",
                    "model": model,
                    "capability": req.capability
                }),
            },
        ))
    }

    async fn invoke_video(
        &self,
        req: ProviderRequest,
        image_to_video: bool,
    ) -> ProviderResultValue<ProviderResult> {
        let prompt = required_string(&req.params, "prompt")?;
        let model = optional_string(&req.params, "model").unwrap_or_else(|| {
            if image_to_video {
                "alibaba/wan-2.6/image-to-video-flash".to_owned()
            } else {
                "bytedance/seedance-v1.5-pro/text-to-video-fast".to_owned()
            }
        });
        let duration = req
            .params
            .get("duration_sec")
            .and_then(Value::as_u64)
            .unwrap_or(5);
        let resolution =
            optional_string(&req.params, "resolution").unwrap_or_else(|| "720P".to_owned());
        let mut body = json!({
            "model": model,
            "prompt": prompt,
            "duration": duration,
            "resolution": resolution,
            "enable_sync_mode": false
        });
        if image_to_video {
            body["image"] = Value::String(required_image_source(&req)?);
        }

        let response = self
            .post_json(
                format!("{}/api/v1/model/generateVideo", self.api_root()),
                body,
            )
            .await?;
        let prediction_id = response
            .pointer("/data/id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProviderError::InvalidResponse("video response missing data.id".to_owned())
            })?
            .to_owned();
        let completed = self.poll_prediction(&prediction_id).await?;
        let output = first_output(&completed)?;
        Ok(single_output_result(
            "video",
            ArtifactPayload {
                kind: ArtifactKind::Video,
                mime: "video/mp4".to_owned(),
                storage_uri: output,
                width: None,
                height: None,
                duration_ms: Some(duration.saturating_mul(1000).min(u32::MAX as u64) as u32),
                meta: json!({
                    "provider": "atlas",
                    "model": model,
                    "prediction_id": prediction_id,
                    "resolution": resolution
                }),
            },
        ))
    }

    async fn poll_prediction(&self, prediction_id: &str) -> ProviderResultValue<Value> {
        let deadline = tokio::time::Instant::now() + self.config.poll_timeout;
        loop {
            let response = self
                .get_json(format!(
                    "{}/api/v1/model/prediction/{}",
                    self.api_root(),
                    prediction_id
                ))
                .await?;
            let data = response.get("data").unwrap_or(&response);
            let status = data.get("status").and_then(Value::as_str).unwrap_or("");
            match status {
                "completed" | "succeeded" => return Ok(data.clone()),
                "failed" => {
                    let message = data
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("Atlas prediction failed");
                    return Err(ProviderError::RequestFailed(message.to_owned()));
                }
                _ if tokio::time::Instant::now() >= deadline => {
                    return Err(ProviderError::RequestFailed(format!(
                        "Atlas prediction `{prediction_id}` timed out"
                    )));
                }
                _ => tokio::time::sleep(self.config.poll_interval).await,
            }
        }
    }
}

#[async_trait]
impl Provider for AtlasProvider {
    fn id(&self) -> &'static str {
        self.config.provider_id
    }

    async fn health(&self) -> ProviderHealth {
        ProviderHealth {
            ok: true,
            message: Some("Atlas API provider configured".to_owned()),
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
        match req.capability.as_str() {
            "chat_completion" => self.invoke_chat(req).await,
            "image_generate" => self.invoke_image(req, false).await,
            "image_edit" => self.invoke_image(req, true).await,
            "text_to_video" => self.invoke_video(req, false).await,
            "image_to_video" => self.invoke_video(req, true).await,
            capability => Err(ProviderError::UnsupportedCapability(capability.to_owned())),
        }
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        Err(ProviderError::CancelUnsupported(handle.provider_task_id))
    }
}

async fn response_json(response: reqwest::Response) -> ProviderResultValue<Value> {
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|err| ProviderError::RequestFailed(err.to_string()))?;
    if !status.is_success() {
        return Err(ProviderError::RequestFailed(format!(
            "HTTP {status}: {}",
            truncate(&text, 512)
        )));
    }
    serde_json::from_str(&text)
        .map_err(|err| ProviderError::InvalidResponse(format!("{err}: {}", truncate(&text, 512))))
}

fn single_output_result(output_name: &str, payload: ArtifactPayload) -> ProviderResult {
    ProviderResult {
        outputs: BTreeMap::from([(output_name.to_owned(), payload)]),
        cost: CostEstimate {
            amount: 0.0,
            currency: "USD".to_owned(),
            estimated: false,
        },
    }
}

fn required_string(params: &Value, key: &str) -> ProviderResultValue<String> {
    optional_string(params, key)
        .ok_or_else(|| ProviderError::InvalidRequest(format!("missing string param `{key}`")))
}

fn optional_string(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn required_image_source(req: &ProviderRequest) -> ProviderResultValue<String> {
    let source = optional_string(&req.params, "image").or_else(|| {
        req.inputs
            .get("image")
            .map(|input| input.storage_uri.clone())
    });
    let source = source.ok_or_else(|| {
        ProviderError::InvalidRequest("missing `image` param or connected image input".to_owned())
    })?;
    if source.starts_with("http://")
        || source.starts_with("https://")
        || source.starts_with("data:")
    {
        return Ok(source);
    }
    Err(ProviderError::InvalidRequest(
        "Atlas image input must be an http(s) URL or data URI".to_owned(),
    ))
}

fn first_output(response: &Value) -> ProviderResultValue<String> {
    let data = response.get("data").unwrap_or(response);
    if let Some(output) = data
        .get("outputs")
        .and_then(Value::as_array)
        .and_then(|outputs| outputs.iter().find_map(Value::as_str))
    {
        return Ok(output.to_owned());
    }
    if let Some(output) = data
        .get("urls")
        .and_then(Value::as_object)
        .and_then(|urls| urls.values().find_map(Value::as_str))
    {
        return Ok(output.to_owned());
    }
    Err(ProviderError::InvalidResponse(
        "Atlas response missing data.outputs or data.urls".to_owned(),
    ))
}

fn normalize_api_base(api_base: &str) -> String {
    let trimmed = api_base.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        DEFAULT_ATLAS_API_BASE.to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn env_duration_ms(key: &str, default_ms: u64) -> Duration {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_millis(default_ms))
}

fn env_duration_secs(key: &str, default_secs: u64) -> Duration {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(default_secs))
}

fn atlas_extra_header() -> Option<(String, String)> {
    let name = std::env::var("ATLAS_API_EXTRA_HEADER_NAME")
        .or_else(|_| std::env::var("ATLAS_AUTH_EXTRA_HEADER_NAME"))
        .ok()?;
    let value = std::env::var("ATLAS_API_EXTRA_HEADER_VALUE")
        .or_else(|_| std::env::var("ATLAS_AUTH_EXTRA_HEADER_VALUE"))
        .ok()?;
    let name = name.trim().to_owned();
    let value = value.trim().to_owned();
    if name.is_empty() || value.is_empty() {
        None
    } else {
        Some((name, value))
    }
}

fn atlas_account_id() -> Option<String> {
    std::env::var("ATLAS_API_ACCOUNT_ID")
        .or_else(|_| std::env::var("ATLAS_ACCOUNT_ID"))
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn atlas_api_key(api_base: &str) -> Option<String> {
    let key_order: &[&str] = if api_base.contains("api.dev.") {
        &["ATLAS_DEV_API_KEY", "ATLAS_API_KEY", "LLM_API_KEY"]
    } else {
        &["ATLAS_API_KEY", "LLM_API_KEY", "ATLAS_DEV_API_KEY"]
    };
    key_order.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

fn truncate(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_owned();
    }
    format!("{}...", &value[..max_len])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_outputs_accept_outputs_or_urls() {
        assert_eq!(
            first_output(&json!({ "data": { "outputs": ["https://cdn.example/image.png"] } }))
                .expect("output"),
            "https://cdn.example/image.png"
        );
        assert_eq!(
            first_output(
                &json!({ "data": { "urls": { "video": "https://cdn.example/out.mp4" } } })
            )
            .expect("url"),
            "https://cdn.example/out.mp4"
        );
    }

    #[test]
    fn atlas_rejects_local_workspace_image_inputs() {
        let req = ProviderRequest {
            provider: "atlas".to_owned(),
            capability: "image_to_video".to_owned(),
            node_id: "video".to_owned(),
            run_id: "run".to_owned(),
            inputs: BTreeMap::from([(
                "image".to_owned(),
                helixflow_gateway::ArtifactRef {
                    artifact_id: "artifact".to_owned(),
                    storage_uri: "workspace://uploads/local.png".to_owned(),
                },
            )]),
            params: json!({ "prompt": "move" }),
        };

        let err = required_image_source(&req).expect_err("workspace URI should fail");

        assert!(err.to_string().contains("http(s) URL or data URI"));
    }

    #[tokio::test]
    async fn no_configured_provider_explains_atlas_env() {
        let provider = NoConfiguredProvider;
        let health = provider.health().await;

        assert!(!health.ok);
        assert!(health.message.expect("message").contains("ATLAS_API_KEY"));
    }
}
