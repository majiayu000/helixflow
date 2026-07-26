use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{
    ArtifactContent, ArtifactKind, ArtifactPayload, CostEstimate, Provider, ProviderCapability,
    ProviderCatalog, ProviderError, ProviderHealth, ProviderRequest, ProviderResult,
    ProviderResultValue, ProviderTaskHandle, wired_or_param_string,
};

const DEFAULT_ATLAS_API_BASE: &str = "https://api.atlascloud.ai/v1";

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
    pub fn atlas(api_key: String, api_base: String) -> Self {
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
    /// In-flight remote prediction ids by run id (HF-011).
    in_flight: std::sync::Arc<tokio::sync::Mutex<BTreeMap<String, Vec<ProviderTaskHandle>>>>,
}

impl AtlasProvider {
    pub fn from_env() -> Option<Self> {
        let api_base =
            std::env::var("ATLAS_API_BASE").unwrap_or_else(|_| DEFAULT_ATLAS_API_BASE.to_owned());
        let api_key = atlas_api_key(&api_base)?;
        let config = ApiProviderConfig::atlas(api_key, api_base);
        // Reject unparseable header configuration at startup instead of
        // silently dropping it and failing with an auth error at run time
        // (HF-037).
        if let Err(message) = validate_header_config(&config) {
            eprintln!("[ATLAS CONFIG ERROR] {message}; Atlas provider disabled");
            return None;
        }
        Some(Self::new(config))
    }

    pub fn new(config: ApiProviderConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            in_flight: std::sync::Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
        }
    }

    async fn register_in_flight(&self, run_id: &str, prediction_id: &str) {
        self.in_flight
            .lock()
            .await
            .entry(run_id.to_owned())
            .or_default()
            .push(ProviderTaskHandle {
                provider: self.config.provider_id.to_owned(),
                provider_task_id: prediction_id.to_owned(),
            });
    }

    async fn clear_in_flight(&self, run_id: &str, prediction_id: &str) {
        let mut in_flight = self.in_flight.lock().await;
        if let Some(handles) = in_flight.get_mut(run_id) {
            handles.retain(|handle| handle.provider_task_id != prediction_id);
            if handles.is_empty() {
                in_flight.remove(run_id);
            }
        }
    }

    pub fn api_base(&self) -> &str {
        &self.config.api_base
    }

    pub fn catalog_value() -> ProviderCatalog {
        ProviderCatalog {
            provider: "atlas".to_owned(),
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
        if let Some(account_id) = &self.config.account_id
            && let Ok(value) = reqwest::header::HeaderValue::from_str(account_id)
        {
            headers.insert("x-account-id", value);
        }
        if let Some((name, value)) = &self.config.extra_header
            && let Ok(name) = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            && let Ok(value) = reqwest::header::HeaderValue::from_str(value)
        {
            headers.insert(name, value);
        }
        headers
    }

    async fn invoke_chat(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        let prompt = wired_or_param_string(&req, "text", "prompt")?;
        // GH130 T4: the model comes only from the run's resolved binding.
        let model = req
            .operation_id
            .clone()
            .ok_or_else(|| ProviderError::ModelUnresolved {
                capability: req.capability.clone(),
            })?;
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
                    "max_tokens": req.params.get("max_tokens").and_then(Value::as_u64).unwrap_or(4096),
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
                storage_uri: format!("provider://atlas/{}/{}/prompt.txt", req.run_id, req.node_id),
                content: ArtifactContent::InlineBytes {
                    bytes: content.as_bytes().to_vec(),
                    ext_hint: Some("txt".to_owned()),
                },
                width: None,
                height: None,
                duration_ms: None,
                meta: json!({ "provider": "atlas", "model": model, "capability": req.capability }),
            },
        ))
    }

    async fn invoke_image(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        let prompt = wired_or_param_string(&req, "prompt", "prompt")?;
        // GH130 T4: the model comes only from the run's resolved binding.
        let model = req
            .operation_id
            .clone()
            .ok_or_else(|| ProviderError::ModelUnresolved {
                capability: req.capability.clone(),
            })?;
        let response = self
            .post_json(
                format!("{}/api/v1/model/generateImage", self.api_root()),
                json!({
                    "model": model,
                    "prompt": prompt,
                    "enable_sync_mode": true,
                    "output_format": optional_string(&req.params, "output_format").unwrap_or_else(|| "png".to_owned()),
                    "num_images": req.params.get("num_images").and_then(Value::as_u64).unwrap_or(1),
                    "aspect_ratio": optional_string(&req.params, "aspect_ratio").unwrap_or_else(|| "1:1".to_owned())
                }),
            )
            .await?;
        let output = first_output(&response)?;
        Ok(remote_output_result(
            "image",
            ArtifactKind::Image,
            "image/png",
            output,
            json!({ "provider": "atlas", "model": model, "capability": req.capability }),
        ))
    }

    async fn invoke_video(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        let prompt = wired_or_param_string(&req, "prompt", "prompt")?;
        // GH130 T4: the model comes only from the run's resolved binding.
        let model = req
            .operation_id
            .clone()
            .ok_or_else(|| ProviderError::ModelUnresolved {
                capability: req.capability.clone(),
            })?;
        let duration = req
            .params
            .get("duration_sec")
            .and_then(Value::as_u64)
            .unwrap_or(5);
        let response = self
            .post_json(
                format!("{}/api/v1/model/generateVideo", self.api_root()),
                json!({
                    "model": model,
                    "prompt": prompt,
                    "duration": duration,
                    "resolution": optional_string(&req.params, "resolution").unwrap_or_else(|| "720P".to_owned()),
                    "enable_sync_mode": false
                }),
            )
            .await?;
        let prediction_id = response
            .pointer("/data/id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProviderError::InvalidResponse("video response missing data.id".to_owned())
            })?
            .to_owned();
        self.register_in_flight(&req.run_id, &prediction_id).await;
        let completed = self.poll_prediction(&prediction_id).await;
        self.clear_in_flight(&req.run_id, &prediction_id).await;
        let completed = completed?;
        let output = first_output(&completed)?;
        let mut result = remote_output_result(
            "video",
            ArtifactKind::Video,
            "video/mp4",
            output,
            json!({
                "provider": "atlas",
                "model": model,
                "prediction_id": prediction_id,
                "capability": req.capability
            }),
        );
        if let Some(payload) = result.outputs.get_mut("video") {
            payload.duration_ms = Some(duration.saturating_mul(1000).min(u32::MAX as u64) as u32);
        }
        Ok(result)
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
                    return Err(ProviderError::RequestFailed(
                        "Atlas prediction timed out".to_owned(),
                    ));
                }
                _ => tokio::time::sleep(self.config.poll_interval).await,
            }
        }
    }
}

#[async_trait]
impl Provider for AtlasProvider {
    fn id(&self) -> &str {
        self.config.provider_id
    }

    fn config_fingerprint(&self, _provider_id: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"atlas-catalog-v1");
        hasher.update(self.config.api_base.as_bytes());
        hasher.update([0]);
        hasher.update(self.config.account_id.as_deref().unwrap_or("").as_bytes());
        hasher.update([0]);
        if let Some((name, _)) = &self.config.extra_header {
            hasher.update(name.as_bytes());
        }
        format!("sha256:{:x}", hasher.finalize())
    }

    async fn health(&self) -> ProviderHealth {
        // Real reachability + auth probe against the OpenAI-compatible
        // models endpoint instead of "key env var is set" (HF-010).
        let response = self
            .client
            .get(format!("{}/models", self.llm_api_base()))
            .bearer_auth(&self.config.api_key)
            .headers(self.extra_headers())
            .timeout(Duration::from_secs(5))
            .send()
            .await;
        match response {
            Ok(response) if response.status().is_success() => ProviderHealth {
                ok: true,
                message: Some("Atlas API reachable".to_owned()),
            },
            Ok(response) => ProviderHealth {
                ok: false,
                message: Some(format!(
                    "Atlas API returned HTTP {}",
                    response.status().as_u16()
                )),
            },
            Err(err) => ProviderHealth {
                ok: false,
                message: Some(crate::safe_provider_message(&format!(
                    "Atlas API unreachable: {err}"
                ))),
            },
        }
    }

    async fn active_handles(&self, run_id: &str) -> Vec<ProviderTaskHandle> {
        self.in_flight
            .lock()
            .await
            .get(run_id)
            .cloned()
            .unwrap_or_default()
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
            unknown: true,
        })
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        self.ensure_provider(&req)?;
        match req.capability.as_str() {
            "prompt_writer" => self.invoke_chat(req).await,
            "image_generate" => self.invoke_image(req).await,
            "text_to_video" => self.invoke_video(req).await,
            capability => Err(ProviderError::UnsupportedCapability(capability.to_owned())),
        }
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        // The Atlas API only exposes submit (generateImage/generateVideo) and
        // poll (prediction/{id}) endpoints; there is no cancel endpoint, so an
        // already-submitted prediction keeps running (and billing) remotely.
        // Fail with CancelUnsupported so interrupt_run surfaces the billing
        // consequence to the user instead of pretending the task stopped.
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
            "HTTP {}: {}",
            status.as_u16(),
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
            // Atlas does not report per-call cost; record the charge as
            // unknown instead of a fake confirmed $0 (HF-004).
            estimated: true,
            unknown: true,
        },
    }
}

fn remote_output_result(
    output_name: &str,
    kind: ArtifactKind,
    mime: &str,
    url: String,
    meta: Value,
) -> ProviderResult {
    single_output_result(
        output_name,
        ArtifactPayload {
            kind,
            mime: mime.to_owned(),
            storage_uri: format!("provider://atlas/{}", output_name),
            content: ArtifactContent::RemoteUrl { url },
            width: None,
            height: None,
            duration_ms: None,
            meta,
        },
    )
}

fn optional_string(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
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
        "Atlas response missing outputs or urls".to_owned(),
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

fn validate_header_config(config: &ApiProviderConfig) -> Result<(), String> {
    if let Some(account_id) = &config.account_id
        && reqwest::header::HeaderValue::from_str(account_id).is_err()
    {
        return Err("ATLAS account id is not a valid header value".to_owned());
    }
    if let Some((name, value)) = &config.extra_header {
        if reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_err() {
            return Err(format!("ATLAS extra header name `{name}` is invalid"));
        }
        if reqwest::header::HeaderValue::from_str(value).is_err() {
            return Err(format!("ATLAS extra header `{name}` has an invalid value"));
        }
    }
    Ok(())
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
    value.chars().take(max_len).collect::<String>()
}

#[cfg(test)]
mod tests;
