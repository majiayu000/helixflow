use std::time::Duration;

use crate::{ProviderError, ProviderResultValue};

pub(super) const DEFAULT_ATLAS_API_BASE: &str = "https://api.atlascloud.ai/v1";

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

fn normalize_api_base(api_base: &str) -> String {
    let trimmed = api_base.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        DEFAULT_ATLAS_API_BASE.to_owned()
    } else {
        trimmed.to_owned()
    }
}

pub(super) fn canonical_origin(api_base: &str) -> ProviderResultValue<String> {
    let parsed = reqwest::Url::parse(api_base).map_err(|_| {
        ProviderError::InvalidRequest("Atlas API base is not a valid URL".to_owned())
    })?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(ProviderError::InvalidRequest(
            "Atlas API base has an unsafe origin".to_owned(),
        ));
    }
    Ok(parsed.origin().ascii_serialization())
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

pub(super) fn validate_header_config(config: &ApiProviderConfig) -> Result<(), String> {
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

pub(super) fn atlas_api_key(api_base: &str) -> Option<String> {
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
