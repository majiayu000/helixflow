use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{
    ArtifactContent, ArtifactKind, ArtifactPayload, CostEstimate, Provider, ProviderCapability,
    ProviderCatalog, ProviderError, ProviderHealth, ProviderRequest, ProviderResult,
    ProviderResultValue, ProviderTaskHandle, wired_or_param_string,
};

const DEFAULT_FAL_API_BASE: &str = "https://queue.fal.run";
const DEFAULT_FAL_IMAGE_MODEL: &str = "fal-ai/nano-banana-2";

#[derive(Debug, Clone)]
pub struct FalProviderConfig {
    pub api_key: String,
    pub api_base: String,
    pub image_model: String,
    pub poll_interval: Duration,
    pub poll_timeout: Duration,
}

impl FalProviderConfig {
    pub fn new(api_key: String, api_base: String, image_model: String) -> Self {
        Self {
            api_key,
            api_base: normalize_api_base(&api_base),
            image_model: normalize_model_path(&image_model)
                .unwrap_or_else(|| DEFAULT_FAL_IMAGE_MODEL.to_owned()),
            poll_interval: env_duration_ms("HELIXFLOW_FAL_POLL_INTERVAL_MS", 2_000),
            poll_timeout: env_duration_secs("HELIXFLOW_FAL_POLL_TIMEOUT_SECS", 300),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FalProvider {
    config: FalProviderConfig,
    client: reqwest::Client,
    /// In-flight remote tasks by run id; provider_task_id is the validated
    /// status URL so cancel can be derived from it (HF-011).
    in_flight: std::sync::Arc<tokio::sync::Mutex<BTreeMap<String, Vec<ProviderTaskHandle>>>>,
}

impl FalProvider {
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("FAL_KEY")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())?;
        let api_base =
            std::env::var("FAL_API_BASE").unwrap_or_else(|_| DEFAULT_FAL_API_BASE.to_owned());
        let image_model =
            std::env::var("FAL_IMAGE_MODEL").unwrap_or_else(|_| DEFAULT_FAL_IMAGE_MODEL.to_owned());
        Some(Self::new(FalProviderConfig::new(
            api_key,
            api_base,
            image_model,
        )))
    }

    pub fn new(config: FalProviderConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            in_flight: std::sync::Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
        }
    }

    async fn register_in_flight(&self, run_id: &str, status_url: &str) -> ProviderTaskHandle {
        let handle = ProviderTaskHandle {
            provider: "fal".to_owned(),
            provider_task_id: status_url.to_owned(),
        };
        self.in_flight
            .lock()
            .await
            .entry(run_id.to_owned())
            .or_default()
            .push(handle.clone());
        handle
    }

    async fn clear_in_flight(&self, run_id: &str, status_url: &str) {
        let mut in_flight = self.in_flight.lock().await;
        if let Some(handles) = in_flight.get_mut(run_id) {
            handles.retain(|handle| handle.provider_task_id != status_url);
            if handles.is_empty() {
                in_flight.remove(run_id);
            }
        }
    }

    pub fn catalog_value() -> ProviderCatalog {
        ProviderCatalog {
            provider: "fal".to_owned(),
            capabilities: BTreeMap::from([(
                "image_generate".to_owned(),
                ProviderCapability {
                    artifact_kind: ArtifactKind::Image,
                    output_name: "image".to_owned(),
                    mime: "image/png".to_owned(),
                },
            )]),
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

    async fn post_json(&self, url: String, body: Value) -> ProviderResultValue<Value> {
        let response = self
            .client
            .post(url)
            .header("authorization", format!("Key {}", self.config.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|err| {
                ProviderError::RequestFailed(redact_sensitive(
                    &err.to_string(),
                    &self.config.api_key,
                ))
            })?;
        response_json(response, &self.config.api_key).await
    }

    async fn get_json(&self, url: String) -> ProviderResultValue<Value> {
        let response = self
            .client
            .get(url)
            .header("authorization", format!("Key {}", self.config.api_key))
            .send()
            .await
            .map_err(|err| {
                ProviderError::RequestFailed(redact_sensitive(
                    &err.to_string(),
                    &self.config.api_key,
                ))
            })?;
        response_json(response, &self.config.api_key).await
    }

    async fn invoke_image(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        let prompt = wired_or_param_string(&req, "prompt", "prompt")?;
        let model = optional_string(&req.params, "model")
            .and_then(|value| normalize_model_path(&value))
            .unwrap_or_else(|| self.config.image_model.clone());
        let body = image_request_body(&req.params, prompt);
        let submit = self
            .post_json(format!("{}/{}", self.config.api_base, model), body)
            .await?;
        let request_id = submit
            .get("request_id")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        let response_url = validate_callback_url(
            response_url(&submit, &self.config.api_base, &model, &request_id),
            &self.config.api_base,
        )?;
        let status_url = validate_callback_url(
            status_url(&submit, &self.config.api_base, &model, &request_id),
            &self.config.api_base,
        )?;
        drop(self.register_in_flight(&req.run_id, &status_url).await);
        let poll_result = self.poll_status(&status_url).await;
        self.clear_in_flight(&req.run_id, &status_url).await;
        poll_result?;
        let result = self.get_json(response_url).await?;
        let image = first_image(&result)?;

        Ok(ProviderResult {
            outputs: BTreeMap::from([(
                "image".to_owned(),
                ArtifactPayload {
                    kind: ArtifactKind::Image,
                    mime: image.mime,
                    storage_uri: format!("provider://fal/{}/{}/image", req.run_id, req.node_id),
                    content: image.content,
                    width: image.width,
                    height: image.height,
                    duration_ms: None,
                    meta: json!({
                        "provider": "fal",
                        "model": model,
                        "request_id": request_id,
                        "capability": req.capability
                    }),
                },
            )]),
            cost: CostEstimate {
                amount: 0.0,
                currency: "USD".to_owned(),
                // FAL does not report per-call cost; the charge is unknown,
                // never a confirmed $0 (HF-004).
                estimated: true,
                unknown: true,
            },
        })
    }

    async fn poll_status(&self, status_url: &str) -> ProviderResultValue<()> {
        let deadline = tokio::time::Instant::now() + self.config.poll_timeout;
        loop {
            let response = self.get_json(status_url.to_owned()).await?;
            let status = response.get("status").and_then(Value::as_str).unwrap_or("");
            if let Some(error) = response.get("error").and_then(Value::as_str) {
                return Err(ProviderError::RequestFailed(redact_sensitive(
                    error,
                    &self.config.api_key,
                )));
            }
            match status {
                "COMPLETED" => return Ok(()),
                "IN_QUEUE" | "IN_PROGRESS" | "" if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(self.config.poll_interval).await;
                }
                "IN_QUEUE" | "IN_PROGRESS" | "" => {
                    return Err(ProviderError::RequestFailed(
                        "fal request timed out".to_owned(),
                    ));
                }
                other => {
                    return Err(ProviderError::RequestFailed(format!(
                        "fal request ended with status `{other}`"
                    )));
                }
            }
        }
    }
}

#[async_trait]
impl Provider for FalProvider {
    fn id(&self) -> &str {
        "fal"
    }

    fn config_fingerprint(&self, _provider_id: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"fal-catalog-v1");
        hasher.update(self.config.api_base.as_bytes());
        hasher.update([0]);
        hasher.update(self.config.image_model.as_bytes());
        format!("sha256:{:x}", hasher.finalize())
    }

    async fn health(&self) -> ProviderHealth {
        // Real reachability + auth probe instead of "key env var is set"
        // (HF-010). Any HTTP response means the endpoint is reachable;
        // 401/403 means the key is rejected.
        let response = self
            .client
            .get(self.config.api_base.clone())
            .header("authorization", format!("Key {}", self.config.api_key))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;
        match response {
            Ok(response)
                if response.status() == reqwest::StatusCode::UNAUTHORIZED
                    || response.status() == reqwest::StatusCode::FORBIDDEN =>
            {
                ProviderHealth {
                    ok: false,
                    message: Some(format!(
                        "fal.ai rejected the API key (HTTP {})",
                        response.status().as_u16()
                    )),
                }
            }
            Ok(_) => ProviderHealth {
                ok: true,
                message: Some("fal.ai endpoint reachable".to_owned()),
            },
            Err(err) => ProviderHealth {
                ok: false,
                message: Some(redact_sensitive(
                    &format!("fal.ai endpoint unreachable: {err}"),
                    &self.config.api_key,
                )),
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
        if req.capability != "image_generate" {
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
            "image_generate" => self.invoke_image(req).await,
            capability => Err(ProviderError::UnsupportedCapability(capability.to_owned())),
        }
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        // fal queue API: PUT {status_url}/cancel. provider_task_id holds the
        // validated status URL for the in-flight request (HF-011).
        let cancel_url = format!(
            "{}/cancel",
            handle.provider_task_id.trim_end_matches("/status")
        );
        drop(validate_callback_url(
            cancel_url.clone(),
            &self.config.api_base,
        )?);
        let response = self
            .client
            .put(cancel_url)
            .header("authorization", format!("Key {}", self.config.api_key))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|err| {
                ProviderError::RequestFailed(redact_sensitive(
                    &err.to_string(),
                    &self.config.api_key,
                ))
            })?;
        if !response.status().is_success() {
            return Err(ProviderError::RequestFailed(format!(
                "fal cancel returned HTTP {}",
                response.status().as_u16()
            )));
        }
        Ok(())
    }
}

async fn response_json(response: reqwest::Response, api_key: &str) -> ProviderResultValue<Value> {
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|err| ProviderError::RequestFailed(redact_sensitive(&err.to_string(), api_key)))?;
    let redacted = redact_sensitive(&text, api_key);
    if !status.is_success() {
        let message = match status.as_u16() {
            401 | 403 => "fal authentication failed".to_owned(),
            429 => "fal rate limit exceeded".to_owned(),
            code => format!("HTTP {code}: {}", truncate(&redacted, 512)),
        };
        return Err(ProviderError::RequestFailed(message));
    }
    serde_json::from_str(&redacted).map_err(|err| {
        ProviderError::InvalidResponse(format!("{err}: {}", truncate(&redacted, 512)))
    })
}

fn image_request_body(params: &Value, prompt: String) -> Value {
    let mut body = json!({
        "prompt": prompt,
        "num_images": params.get("num_images").and_then(Value::as_u64).unwrap_or(1)
    });
    copy_optional_string(params, &mut body, "aspect_ratio");
    copy_optional_string(params, &mut body, "image_size");
    if let Some(seed) = params.get("seed").and_then(Value::as_u64) {
        body["seed"] = json!(seed);
    }
    body
}

fn copy_optional_string(params: &Value, body: &mut Value, key: &str) {
    if let Some(value) = optional_string(params, key) {
        body[key] = json!(value);
    }
}

fn response_url(submit: &Value, api_base: &str, model: &str, request_id: &str) -> String {
    submit
        .get("response_url")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{api_base}/{model}/requests/{request_id}/response"))
}

fn status_url(submit: &Value, api_base: &str, model: &str, request_id: &str) -> String {
    submit
        .get("status_url")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{api_base}/{model}/requests/{request_id}/status"))
}

fn validate_callback_url(url: String, api_base: &str) -> ProviderResultValue<String> {
    if url == api_base || url.starts_with(&format!("{api_base}/")) {
        return Ok(url);
    }
    Err(ProviderError::InvalidResponse(
        "fal response has unexpected callback URL".to_owned(),
    ))
}

struct FalImage {
    content: ArtifactContent,
    mime: String,
    width: Option<u32>,
    height: Option<u32>,
}

fn first_image(response: &Value) -> ProviderResultValue<FalImage> {
    let image = response
        .get("images")
        .and_then(Value::as_array)
        .and_then(|images| images.first())
        .or_else(|| response.get("image"))
        .ok_or_else(|| ProviderError::InvalidResponse("fal response missing image".to_owned()))?;
    let mime = image
        .get("content_type")
        .or_else(|| image.get("mime"))
        .and_then(Value::as_str)
        .unwrap_or("image/png")
        .to_owned();
    let width = image
        .get("width")
        .and_then(Value::as_u64)
        .map(saturating_u32);
    let height = image
        .get("height")
        .and_then(Value::as_u64)
        .map(saturating_u32);
    if let Some(url) = image.get("url").and_then(Value::as_str) {
        return Ok(FalImage {
            content: ArtifactContent::RemoteUrl {
                url: url.to_owned(),
            },
            mime,
            width,
            height,
        });
    }
    if let Some(bytes) = image.get("bytes").and_then(byte_array) {
        return Ok(FalImage {
            content: ArtifactContent::InlineBytes {
                bytes,
                ext_hint: None,
            },
            mime,
            width,
            height,
        });
    }
    Err(ProviderError::InvalidResponse(
        "fal image is missing url or bytes".to_owned(),
    ))
}

fn byte_array(value: &Value) -> Option<Vec<u8>> {
    value.as_array().and_then(|items| {
        items
            .iter()
            .map(|item| item.as_u64().and_then(|value| u8::try_from(value).ok()))
            .collect()
    })
}

fn optional_string(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn normalize_api_base(api_base: &str) -> String {
    let trimmed = api_base.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        DEFAULT_FAL_API_BASE.to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn normalize_model_path(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches('/');
    (!trimmed.is_empty()
        && trimmed.len() <= 128
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '/' | '.')))
    .then(|| trimmed.to_owned())
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

fn saturating_u32(value: u64) -> u32 {
    value.min(u32::MAX as u64) as u32
}

fn redact_sensitive(value: &str, api_key: &str) -> String {
    let redacted = if api_key.is_empty() {
        value.to_owned()
    } else {
        value.replace(api_key, "[redacted]")
    };
    redact_token_after_prefixes(&redacted, &["Bearer ", "bearer ", "Key ", "key "])
}

fn redact_token_after_prefixes(value: &str, prefixes: &[&str]) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while !rest.is_empty() {
        if let Some(prefix) = prefixes
            .iter()
            .copied()
            .find(|prefix| rest.starts_with(*prefix))
        {
            let prefix_len = prefix.len();
            output.push_str(prefix);
            output.push_str("[redacted]");
            let token_len = rest[prefix_len..]
                .find(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | ',' | '}'))
                .unwrap_or(rest.len() - prefix_len);
            rest = &rest[prefix_len + token_len..];
        } else if let Some(ch) = rest.chars().next() {
            output.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    output
}

fn truncate(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_owned();
    }
    value.chars().take(max_len).collect()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod fingerprint_tests;
