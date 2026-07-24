use std::net::SocketAddr;

use axum::{
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::canvas_ticket::constant_time_eq;

/// Shared auth configuration resolved once at startup (HF-017).
#[derive(Debug, Clone)]
pub(crate) struct AuthConfig {
    token: Option<String>,
}

impl AuthConfig {
    pub(crate) fn from_env() -> Self {
        Self {
            token: std::env::var("HELIXFLOW_AUTH_TOKEN")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty()),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_token(token: Option<&str>) -> Self {
        Self {
            token: token.map(str::to_owned),
        }
    }

    pub(crate) fn required(&self) -> bool {
        self.token.is_some()
    }
}

/// Fail closed: a non-loopback bind address without an auth token would
/// expose every workspace, provider, and paid-run route to the network.
pub(crate) fn validate_bind_auth(bind_addr: &str, auth: &AuthConfig) -> Result<(), String> {
    let parsed: SocketAddr = bind_addr
        .parse()
        .map_err(|err| format!("HELIXFLOW_BIND_ADDR `{bind_addr}` is invalid: {err}"))?;
    if parsed.ip().is_loopback() || auth.required() {
        return Ok(());
    }
    Err(format!(
        "refusing to bind non-loopback address `{bind_addr}` without HELIXFLOW_AUTH_TOKEN; \
         set a token or bind to 127.0.0.1"
    ))
}

pub(crate) async fn require_auth(request: Request, next: Next) -> Response {
    let Some(auth) = request.extensions().get::<AuthConfig>().cloned() else {
        return unauthorized("auth configuration is missing");
    };
    let Some(expected) = auth.token else {
        return next.run(request).await;
    };
    let presented = bearer_token(request.headers()).or_else(|| query_token(request.uri()));
    match presented {
        Some(token) if constant_time_eq(token.as_bytes(), expected.as_bytes()) => {
            next.run(request).await
        }
        _ => unauthorized("missing or invalid auth token"),
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(|token| token.trim().to_owned())
}

fn query_token(uri: &axum::http::Uri) -> Option<String> {
    let query = uri.query()?;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "token" && !value.is_empty()).then(|| value.to_owned())
    })
}

fn unauthorized(message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(json!({ "error": message })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_loopback_bind_without_token_is_rejected() {
        let auth = AuthConfig::with_token(None);
        assert!(validate_bind_auth("0.0.0.0:8787", &auth).is_err());
        assert!(validate_bind_auth("192.168.1.10:8787", &auth).is_err());
    }

    #[test]
    fn loopback_bind_without_token_is_allowed() {
        let auth = AuthConfig::with_token(None);
        assert!(validate_bind_auth("127.0.0.1:8787", &auth).is_ok());
        assert!(validate_bind_auth("[::1]:8787", &auth).is_ok());
    }

    #[test]
    fn non_loopback_bind_with_token_is_allowed() {
        let auth = AuthConfig::with_token(Some("secret"));
        assert!(validate_bind_auth("0.0.0.0:8787", &auth).is_ok());
    }

    #[test]
    fn invalid_bind_addr_is_rejected() {
        let auth = AuthConfig::with_token(Some("secret"));
        assert!(validate_bind_auth("not-an-addr", &auth).is_err());
    }
}
