use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    extract::{Path as AxumPath, State},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::api_error::ApiError;
use crate::app_state::AppState;

const MODE_ENV: &str = "HELIXFLOW_CANVAS_TICKET_MODE";
const SECRET_ENV: &str = "HELIXFLOW_CANVAS_TICKET_SECRET";
const TTL_ENV: &str = "HELIXFLOW_CANVAS_TICKET_TTL_SECS";
const DEFAULT_TTL_SECS: u64 = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
enum CanvasTicketMode {
    Disabled,
    Required,
}

#[derive(Debug, Clone)]
struct CanvasTicketConfig {
    mode: CanvasTicketMode,
    secret: Option<String>,
    ttl_secs: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CanvasTicketResponse {
    mode: &'static str,
    ticket: Option<String>,
    expires_at: Option<i64>,
}

pub(crate) async fn create_canvas_ticket(
    AxumPath(canvas_id): AxumPath<String>,
    State(state): State<AppState>,
) -> Result<Json<CanvasTicketResponse>, ApiError> {
    state
        .store
        .workspace(&canvas_id)
        .await
        .map_err(ApiError::store)?;
    Ok(Json(issue_canvas_ticket(
        &load_canvas_ticket_config()?,
        &canvas_id,
        unix_now()?,
    )?))
}

pub(crate) async fn validate_canvas_ws_ticket(
    state: &AppState,
    canvas_id: Option<&str>,
    ticket: Option<&str>,
) -> Result<(), ApiError> {
    let config = load_canvas_ticket_config()?;
    if config.mode == CanvasTicketMode::Disabled {
        return Ok(());
    }
    let canvas_id = canvas_id.ok_or_else(|| ApiError::unauthorized("canvas ticket is required"))?;
    state
        .store
        .workspace(canvas_id)
        .await
        .map_err(ApiError::store)?;
    validate_ticket(
        &config,
        canvas_id,
        ticket.ok_or_else(|| ApiError::unauthorized("canvas ticket is required"))?,
        unix_now()?,
    )
}

fn load_canvas_ticket_config() -> Result<CanvasTicketConfig, ApiError> {
    CanvasTicketConfig::from_values(
        std::env::var(MODE_ENV).ok().as_deref(),
        std::env::var(SECRET_ENV).ok().as_deref(),
        std::env::var(TTL_ENV).ok().as_deref(),
    )
}

impl CanvasTicketConfig {
    fn from_values(
        mode: Option<&str>,
        secret: Option<&str>,
        ttl_secs: Option<&str>,
    ) -> Result<Self, ApiError> {
        let mode = match mode
            .unwrap_or("disabled")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "" | "0" | "off" | "false" | "disabled" => CanvasTicketMode::Disabled,
            "1" | "on" | "true" | "enabled" | "required" => CanvasTicketMode::Required,
            value => {
                return Err(ApiError::server_error(format!(
                    "invalid {MODE_ENV} value `{value}`"
                )));
            }
        };
        let ttl_secs = match ttl_secs {
            Some(raw) if !raw.trim().is_empty() => raw
                .trim()
                .parse::<u64>()
                .map_err(|_| ApiError::server_error(format!("invalid {TTL_ENV} value")))?
                .max(1),
            _ => DEFAULT_TTL_SECS,
        };
        let secret = secret
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        if mode == CanvasTicketMode::Required && secret.is_none() {
            return Err(ApiError::server_error(format!(
                "{SECRET_ENV} is required when canvas ticket mode is required"
            )));
        }
        Ok(Self {
            mode,
            secret,
            ttl_secs,
        })
    }
}

fn issue_canvas_ticket(
    config: &CanvasTicketConfig,
    canvas_id: &str,
    now: i64,
) -> Result<CanvasTicketResponse, ApiError> {
    if config.mode == CanvasTicketMode::Disabled {
        return Ok(CanvasTicketResponse {
            mode: "disabled",
            ticket: None,
            expires_at: None,
        });
    }
    let expires_at = now
        .checked_add(config.ttl_secs as i64)
        .ok_or_else(|| ApiError::server_error("canvas ticket expiry overflow"))?;
    let nonce = Uuid::now_v7();
    let body = format!("{canvas_id}:{expires_at}:{nonce}");
    let signature = sign_body(config, &body)?;
    Ok(CanvasTicketResponse {
        mode: "required",
        ticket: Some(format!("{body}:{signature}")),
        expires_at: Some(expires_at),
    })
}

fn validate_ticket(
    config: &CanvasTicketConfig,
    canvas_id: &str,
    ticket: &str,
    now: i64,
) -> Result<(), ApiError> {
    if config.mode == CanvasTicketMode::Disabled {
        return Ok(());
    }
    let (body, signature) = ticket
        .rsplit_once(':')
        .ok_or_else(|| ApiError::unauthorized("invalid canvas ticket"))?;
    let mut parts = body.split(':');
    let ticket_canvas_id = parts
        .next()
        .ok_or_else(|| ApiError::unauthorized("invalid canvas ticket"))?;
    let expires_at = parts
        .next()
        .ok_or_else(|| ApiError::unauthorized("invalid canvas ticket"))?
        .parse::<i64>()
        .map_err(|_| ApiError::unauthorized("invalid canvas ticket"))?;
    let nonce = parts
        .next()
        .ok_or_else(|| ApiError::unauthorized("invalid canvas ticket"))?;
    if parts.next().is_some() || nonce.is_empty() || ticket_canvas_id != canvas_id {
        return Err(ApiError::unauthorized("invalid canvas ticket"));
    }
    if expires_at < now {
        return Err(ApiError::unauthorized("canvas ticket expired"));
    }
    let expected = sign_body(config, body)?;
    if !constant_time_eq(expected.as_bytes(), signature.as_bytes()) {
        return Err(ApiError::unauthorized("invalid canvas ticket"));
    }
    Ok(())
}

fn sign_body(config: &CanvasTicketConfig, body: &str) -> Result<String, ApiError> {
    let secret = config
        .secret
        .as_deref()
        .ok_or_else(|| ApiError::server_error("canvas ticket secret is not configured"))?;
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update([0]);
    hasher.update(body.as_bytes());
    Ok(hex_digest(hasher.finalize().as_ref()))
}

fn unix_now() -> Result<i64, ApiError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .map_err(|err| ApiError::server_error(format!("system clock before UNIX epoch: {err}")))
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right.iter())
        .fold(0_u8, |diff, (left, right)| diff | (left ^ right))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_config_returns_no_ticket() {
        let config = CanvasTicketConfig::from_values(None, None, None).expect("config");
        let response = issue_canvas_ticket(&config, "ws_test", 100).expect("ticket");

        assert_eq!(response.mode, "disabled");
        assert_eq!(response.ticket, None);
        assert_eq!(response.expires_at, None);
    }

    #[test]
    fn required_ticket_validates_canvas_and_expiry() {
        let config =
            CanvasTicketConfig::from_values(Some("required"), Some("test-secret"), Some("60"))
                .expect("config");
        let response = issue_canvas_ticket(&config, "ws_test", 100).expect("ticket");
        let ticket = response.ticket.expect("required ticket");

        validate_ticket(&config, "ws_test", &ticket, 120).expect("valid ticket");
        assert!(validate_ticket(&config, "other", &ticket, 120).is_err());
        assert!(validate_ticket(&config, "ws_test", &ticket, 200).is_err());
        assert!(validate_ticket(&config, "ws_test", "bad-ticket", 120).is_err());
    }

    #[test]
    fn required_mode_without_secret_fails_closed() {
        let err = CanvasTicketConfig::from_values(Some("required"), None, None)
            .expect_err("missing secret should fail");

        assert_eq!(err.status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    }
}
