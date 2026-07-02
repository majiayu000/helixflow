use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{
    ArtifactContent, ArtifactKind, ArtifactPayload, CostEstimate, Provider, ProviderCapability,
    ProviderCatalog, ProviderError, ProviderHealth, ProviderRequest, ProviderResult,
    ProviderResultValue, ProviderTaskHandle,
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
        let prompt = required_string(&req.params, "prompt")?;
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
        self.poll_status(&status_url).await?;
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
                estimated: true,
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

    async fn health(&self) -> ProviderHealth {
        ProviderHealth {
            ok: true,
            message: Some("fal.ai provider configured".to_owned()),
        }
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
        Err(ProviderError::CancelUnsupported(handle.provider_task_id))
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
mod tests {
    use super::*;

    fn request(capability: &str) -> ProviderRequest {
        ProviderRequest {
            provider: "fal".to_owned(),
            capability: capability.to_owned(),
            node_id: "image_node".to_owned(),
            run_id: "run_1".to_owned(),
            inputs: BTreeMap::new(),
            params: json!({ "prompt": "a product image", "aspect_ratio": "1:1" }),
        }
    }

    #[test]
    fn fal_catalog_exposes_image_generation_only() {
        let catalog = FalProvider::catalog_value();

        assert!(catalog.capabilities.contains_key("image_generate"));
        assert!(!catalog.capabilities.contains_key("text_to_video"));
    }

    #[tokio::test]
    async fn fal_rejects_unsupported_capability() {
        let provider = FalProvider::new(FalProviderConfig::new(
            "test-key".to_owned(),
            DEFAULT_FAL_API_BASE.to_owned(),
            DEFAULT_FAL_IMAGE_MODEL.to_owned(),
        ));

        let err = provider
            .invoke(request("text_to_video"))
            .await
            .expect_err("unsupported capability");

        assert_eq!(
            err,
            ProviderError::UnsupportedCapability("text_to_video".to_owned())
        );
    }

    #[tokio::test]
    async fn fal_queue_image_generation_returns_remote_image()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            serve_response(
                &listener,
                r#"{"request_id":"req_1","status_url":"http://ADDR/status","response_url":"http://ADDR/response"}"#,
                addr,
            )
            .await?;
            serve_response(
                &listener,
                r#"{"status":"COMPLETED","request_id":"req_1"}"#,
                addr,
            )
            .await?;
            serve_response(
                &listener,
                r#"{"images":[{"url":"https://cdn.example/fal.png","width":768,"height":768,"content_type":"image/png"}]}"#,
                addr,
            )
            .await?;
            Ok::<(), std::io::Error>(())
        });
        let provider = FalProvider::new(FalProviderConfig::new(
            "test-key".to_owned(),
            format!("http://{addr}"),
            DEFAULT_FAL_IMAGE_MODEL.to_owned(),
        ));

        let result = provider
            .invoke(request("image_generate"))
            .await
            .expect("fal image result");
        server.await??;
        let image = result.outputs.get("image").expect("image output");

        assert_eq!(image.kind, ArtifactKind::Image);
        assert_eq!(image.width, Some(768));
        assert_eq!(image.height, Some(768));
        assert!(matches!(image.content, ArtifactContent::RemoteUrl { .. }));
        assert_eq!(image.meta["provider"], "fal");
        Ok(())
    }

    #[tokio::test]
    async fn fal_errors_redact_api_key() -> Result<(), Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            serve_status_response(
                &listener,
                401,
                r#"{"detail":"FAL_KEY test-key was rejected"}"#,
                addr,
            )
            .await?;
            Ok::<(), std::io::Error>(())
        });
        let provider = FalProvider::new(FalProviderConfig::new(
            "test-key".to_owned(),
            format!("http://{addr}"),
            DEFAULT_FAL_IMAGE_MODEL.to_owned(),
        ));

        let err = provider
            .invoke(request("image_generate"))
            .await
            .expect_err("auth failure");
        server.await??;
        let message = err.to_string().to_lowercase();

        assert!(message.contains("authentication"));
        assert!(!message.contains("test-key"));
        assert!(!message.contains("fal_key"));
        Ok(())
    }

    #[tokio::test]
    async fn fal_rate_limit_errors_are_generic() -> Result<(), Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            serve_status_response(
                &listener,
                429,
                r#"{"detail":"Key test-key is over quota"}"#,
                addr,
            )
            .await?;
            Ok::<(), std::io::Error>(())
        });
        let provider = FalProvider::new(FalProviderConfig::new(
            "test-key".to_owned(),
            format!("http://{addr}"),
            DEFAULT_FAL_IMAGE_MODEL.to_owned(),
        ));

        let err = provider
            .invoke(request("image_generate"))
            .await
            .expect_err("rate limit failure");
        server.await??;
        let message = err.to_string().to_lowercase();

        assert!(message.contains("rate limit"));
        assert!(!message.contains("test-key"));
        assert!(!message.contains("over quota"));
        Ok(())
    }

    #[tokio::test]
    async fn fal_status_errors_redact_api_key() -> Result<(), Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            serve_response(
                &listener,
                r#"{"request_id":"req_1","status_url":"http://ADDR/status","response_url":"http://ADDR/response"}"#,
                addr,
            )
            .await?;
            serve_response(
                &listener,
                r#"{"status":"FAILED","error":"Bearer test-key failed upstream"}"#,
                addr,
            )
            .await?;
            Ok::<(), std::io::Error>(())
        });
        let provider = FalProvider::new(FalProviderConfig::new(
            "test-key".to_owned(),
            format!("http://{addr}"),
            DEFAULT_FAL_IMAGE_MODEL.to_owned(),
        ));

        let err = provider
            .invoke(request("image_generate"))
            .await
            .expect_err("status failure");
        server.await??;
        let message = err.to_string().to_lowercase();

        assert!(message.contains("provider message was redacted"));
        assert!(!message.contains("test-key"));
        assert!(!message.contains("bearer"));
        Ok(())
    }

    #[tokio::test]
    async fn fal_rejects_cross_base_callback_urls() -> Result<(), Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            serve_response(
                &listener,
                r#"{"request_id":"req_1","status_url":"https://evil.example/status","response_url":"https://evil.example/response"}"#,
                addr,
            )
            .await?;
            Ok::<(), std::io::Error>(())
        });
        let provider = FalProvider::new(FalProviderConfig::new(
            "test-key".to_owned(),
            format!("http://{addr}"),
            DEFAULT_FAL_IMAGE_MODEL.to_owned(),
        ));

        let err = provider
            .invoke(request("image_generate"))
            .await
            .expect_err("callback URL validation failure");
        server.await??;

        assert!(
            err.to_string()
                .contains("fal response has unexpected callback URL")
        );
        Ok(())
    }

    async fn serve_response(
        listener: &tokio::net::TcpListener,
        body: &str,
        addr: std::net::SocketAddr,
    ) -> std::io::Result<()> {
        serve_status_response(listener, 200, body, addr).await
    }

    async fn serve_status_response(
        listener: &tokio::net::TcpListener,
        status: u16,
        body: &str,
        addr: std::net::SocketAddr,
    ) -> std::io::Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (mut stream, _) = listener.accept().await?;
        let mut buf = [0_u8; 2048];
        let bytes_read = stream.read(&mut buf).await?;
        assert!(bytes_read > 0);
        let body = body.replace("ADDR", &addr.to_string());
        let status_text = if status == 200 { "OK" } else { "Error" };
        let response = format!(
            "HTTP/1.1 {status} {status_text}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await?;
        Ok(())
    }
}
