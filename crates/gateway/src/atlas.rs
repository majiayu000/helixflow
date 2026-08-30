use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{
    ArtifactContent, ArtifactKind, ArtifactPayload, CostEstimate, DurableProviderTask, Provider,
    ProviderCapability, ProviderCatalog, ProviderDispatch, ProviderDispatchFailure,
    ProviderDispatchResult, ProviderError, ProviderHealth, ProviderRecoveryCapabilities,
    ProviderRequest, ProviderResult, ProviderResultValue, ProviderResume, ProviderTaskHandle,
    wired_or_param_string,
};

mod config;

pub use config::ApiProviderConfig;
use config::{DEFAULT_ATLAS_API_BASE, atlas_api_key, canonical_origin, validate_header_config};

const ATLAS_USER_AGENT: &str = concat!("helixflow/", env!("CARGO_PKG_VERSION"));

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
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static(ATLAS_USER_AGENT),
        );
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

    async fn dispatch_video(
        &self,
        req: &ProviderRequest,
    ) -> ProviderResultValue<DurableProviderTask> {
        let prompt = if req.capability == "image_to_video" {
            req.input_texts
                .get("prompt")
                .cloned()
                .or_else(|| optional_string(&req.params, "prompt"))
                .unwrap_or_default()
        } else {
            wired_or_param_string(req, "prompt", "prompt")?
        };
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
        let default_resolution = if req.capability == "image_to_video" {
            "720p"
        } else {
            "720P"
        };
        let mut body = json!({
            "model": model,
            "prompt": prompt,
            "duration": duration,
            "resolution": optional_string(&req.params, "resolution").unwrap_or_else(|| default_resolution.to_owned()),
            "enable_sync_mode": false
        });
        if req.capability == "image_to_video" {
            let image =
                optional_string(&req.params, "__helixflow_wired_image").ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "image_to_video requires wired image input".to_owned(),
                    )
                })?;
            body["image"] = Value::String(image);
            body["generate_audio"] = req
                .params
                .get("generate_audio")
                .cloned()
                .unwrap_or(json!(true));
            body["camera_fixed"] = req
                .params
                .get("camera_fixed")
                .cloned()
                .unwrap_or(json!(false));
            body["seed"] = req.params.get("seed").cloned().unwrap_or(json!(-1));
            if let Some(aspect_ratio) = optional_string(&req.params, "aspect_ratio") {
                body["aspect_ratio"] = Value::String(aspect_ratio);
            }
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
        let dispatch_origin = self.dispatch_origin(self.id())?;
        Ok(DurableProviderTask {
            provider: self.id().to_owned(),
            provider_task_id: prediction_id.clone(),
            dispatch_origin,
            recovery_scope_fingerprint: self.recovery_scope_fingerprint(self.id()),
            status_url: Some(format!(
                "{}/api/v1/model/prediction/{prediction_id}",
                self.api_root()
            )),
            result_url: None,
        })
    }

    async fn invoke_video(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        let task = self.dispatch_video(&req).await?;
        self.register_in_flight(&req.run_id, &task.provider_task_id)
            .await;
        let deadline = tokio::time::Instant::now() + self.config.poll_timeout;
        let completed = loop {
            match self.resume(&task, &req).await {
                Ok(ProviderResume::Completed(result)) => break Ok(result),
                Ok(ProviderResume::Failed { reason_code, .. }) => {
                    break Err(ProviderError::RequestRejected(reason_code));
                }
                Ok(ProviderResume::Pending { retry_after_ms })
                    if tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(Duration::from_millis(retry_after_ms)).await;
                }
                Ok(ProviderResume::Pending { .. }) => {
                    break Err(ProviderError::RequestFailed(
                        "Atlas prediction timed out".to_owned(),
                    ));
                }
                Err(error) => break Err(error),
            }
        };
        self.clear_in_flight(&req.run_id, &task.provider_task_id)
            .await;
        completed
    }

    fn completed_video_result(
        &self,
        req: &ProviderRequest,
        completed: &Value,
    ) -> ProviderResultValue<ProviderResult> {
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
        let output = first_output(completed)?;
        let mut result = remote_output_result(
            "video",
            ArtifactKind::Video,
            "video/mp4",
            output,
            json!({
                "provider": "atlas",
                "model": model,
                "capability": req.capability
            }),
        );
        if let Some(payload) = result.outputs.get_mut("video") {
            payload.duration_ms = Some(duration.saturating_mul(1000).min(u32::MAX as u64) as u32);
        }
        Ok(result)
    }

    fn validate_recovery_task(&self, task: &DurableProviderTask) -> ProviderResultValue<()> {
        if task.provider != self.id() {
            return Err(ProviderError::WrongProvider {
                expected: self.id().to_owned(),
                actual: task.provider.clone(),
            });
        }
        if !is_safe_task_id(&task.provider_task_id)
            || task.dispatch_origin != self.dispatch_origin(self.id())?
            || task.recovery_scope_fingerprint != self.recovery_scope_fingerprint(self.id())
        {
            return Err(ProviderError::InvalidRequest(
                "Atlas recovery handle does not match the configured provider scope".to_owned(),
            ));
        }
        let expected_status_url = format!(
            "{}/api/v1/model/prediction/{}",
            self.api_root(),
            task.provider_task_id
        );
        if task.status_url.as_deref() != Some(expected_status_url.as_str())
            || task.result_url.is_some()
        {
            return Err(ProviderError::InvalidRequest(
                "Atlas recovery handle has an invalid status locator".to_owned(),
            ));
        }
        Ok(())
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
        if let Some((name, value)) = &self.config.extra_header {
            hasher.update(name.as_bytes());
            hasher.update([0]);
            hasher.update(value.as_bytes());
        }
        hasher.update([0]);
        hasher.update(self.config.api_key.as_bytes());
        format!("sha256:{}", hex::encode(hasher.finalize()))
    }

    fn dispatch_origin(&self, provider_id: &str) -> ProviderResultValue<String> {
        if provider_id != self.id() {
            return Err(ProviderError::WrongProvider {
                expected: self.id().to_owned(),
                actual: provider_id.to_owned(),
            });
        }
        canonical_origin(&self.config.api_base)
    }

    fn recovery_capabilities(&self, _provider_id: &str) -> ProviderRecoveryCapabilities {
        ProviderRecoveryCapabilities {
            resume: true,
            cancel: false,
        }
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
            "text_to_image" => self.invoke_image(req).await,
            "text_to_video" | "image_to_video" => self.invoke_video(req).await,
            capability => Err(ProviderError::UnsupportedCapability(capability.to_owned())),
        }
    }

    async fn dispatch(&self, req: ProviderRequest) -> ProviderDispatchResult {
        if let Err(error) = self.ensure_provider(&req) {
            return Err(ProviderDispatchFailure::classify(error));
        }
        let result = match req.capability.as_str() {
            "prompt_writer" => self.invoke_chat(req).await.map(ProviderDispatch::Completed),
            "text_to_image" => self
                .invoke_image(req)
                .await
                .map(ProviderDispatch::Completed),
            "text_to_video" | "image_to_video" => self
                .dispatch_video(&req)
                .await
                .map(ProviderDispatch::Accepted),
            capability => Err(ProviderError::UnsupportedCapability(capability.to_owned())),
        };
        result.map_err(ProviderDispatchFailure::classify)
    }

    async fn resume(
        &self,
        task: &DurableProviderTask,
        req: &ProviderRequest,
    ) -> ProviderResultValue<ProviderResume> {
        self.ensure_provider(req)?;
        if !matches!(req.capability.as_str(), "text_to_video" | "image_to_video") {
            return Err(ProviderError::UnsupportedCapability(req.capability.clone()));
        }
        self.validate_recovery_task(task)?;
        let response = self
            .get_json(task.status_url.clone().ok_or_else(|| {
                ProviderError::InvalidRequest(
                    "Atlas recovery handle is missing a status locator".to_owned(),
                )
            })?)
            .await?;
        let data = response.get("data").unwrap_or(&response);
        match data.get("status").and_then(Value::as_str).unwrap_or("") {
            "completed" | "succeeded" => self
                .completed_video_result(req, data)
                .map(ProviderResume::Completed),
            "failed" => Ok(ProviderResume::failed("PROVIDER_REMOTE_FAILED")),
            _ => Ok(ProviderResume::Pending {
                retry_after_ms: self.config.poll_interval.as_millis().min(u64::MAX as u128) as u64,
            }),
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
        return Err(ProviderError::RequestRejected(format!(
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

fn is_safe_task_id(task_id: &str) -> bool {
    !task_id.is_empty()
        && task_id.len() <= 256
        && task_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn truncate(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        return value.to_owned();
    }
    value.chars().take(max_len).collect::<String>()
}

#[cfg(test)]
mod tests;
