use std::net::SocketAddr;

use axum::{
    Extension, Form,
    extract::Request,
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::canvas_ticket::constant_time_eq;

/// Shared auth configuration resolved once at startup (HF-017).
const SESSION_COOKIE_NAME: &str = "helixflow_session";

#[derive(Clone)]
pub(crate) struct AuthConfig {
    token: Option<String>,
    session_value: Option<String>,
    secure_cookie: bool,
}

impl AuthConfig {
    pub(crate) fn from_env(bind_addr: &str) -> Self {
        Self::new(
            std::env::var("HELIXFLOW_AUTH_TOKEN")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty()),
            secure_cookie_for_bind(bind_addr),
        )
    }

    #[cfg(test)]
    pub(crate) fn with_token(token: Option<&str>) -> Self {
        Self::new(token.map(str::to_owned), false)
    }

    pub(crate) fn required(&self) -> bool {
        self.token.is_some()
    }

    fn new(token: Option<String>, secure_cookie: bool) -> Self {
        let session_value = token.as_deref().map(derive_session_value);
        Self {
            token,
            session_value,
            secure_cookie,
        }
    }

    fn session_value(&self) -> &str {
        self.session_value.as_deref().unwrap_or("")
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
    if is_public_auth_route(request.method(), request.uri().path()) {
        return next.run(request).await;
    }
    if !auth.required() {
        return next.run(request).await;
    }
    if request_is_authorized(&auth, request.headers(), request.uri()) {
        next.run(request).await
    } else if request.method() == Method::GET
        && !request.uri().path().starts_with("/api/")
        && request.uri().path() != "/ws"
    {
        Redirect::temporary("/login").into_response()
    } else {
        unauthorized("missing or invalid auth token")
    }
}

pub(crate) async fn login_page() -> Response {
    let mut response = Html(LOGIN_PAGE).into_response();
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'",
        ),
    );
    response
}

#[derive(Deserialize)]
pub(crate) struct AuthSessionRequest {
    token: String,
}

pub(crate) async fn create_auth_session(
    Extension(auth): Extension<AuthConfig>,
    Form(input): Form<AuthSessionRequest>,
) -> Response {
    let Some(expected) = auth.token.as_deref() else {
        return Redirect::to("/").into_response();
    };
    if !constant_time_eq(input.token.trim().as_bytes(), expected.as_bytes()) {
        return unauthorized("invalid deployment token");
    }
    let cookie = session_cookie_header(&auth, auth.secure_cookie);
    let mut response = Redirect::to("/").into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
        response
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({ "error": "failed to create browser session" })),
        )
            .into_response()
    }
}

fn is_public_auth_route(method: &Method, path: &str) -> bool {
    (*method == Method::GET && path == "/login")
        || (*method == Method::POST && path == "/api/auth/session")
}

fn request_is_authorized(auth: &AuthConfig, headers: &HeaderMap, uri: &axum::http::Uri) -> bool {
    let Some(expected_token) = auth.token.as_deref() else {
        return true;
    };
    if let Some(token) = presented_token(headers, uri)
        && constant_time_eq(token.as_bytes(), expected_token.as_bytes())
    {
        return true;
    }
    session_cookie(headers).is_some_and(|session| {
        constant_time_eq(session.as_bytes(), auth.session_value().as_bytes())
    })
}

/// Tokens in query strings leak through logs, history, and referrers. CLI
/// clients use a bearer header; browsers use the derived HttpOnly session
/// cookie issued by the login route.
fn presented_token(headers: &HeaderMap, _uri: &axum::http::Uri) -> Option<String> {
    bearer_token(headers)
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(|token| token.trim().to_owned())
}

fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(name, value)| {
            (name == SESSION_COOKIE_NAME && !value.is_empty()).then_some(value)
        })
}

fn derive_session_value(token: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"helixflow-browser-session-v1\0");
    digest.update(token.as_bytes());
    hex::encode(digest.finalize())
}

fn session_cookie_header(auth: &AuthConfig, secure: bool) -> String {
    let secure_attribute = if secure { "; Secure" } else { "" };
    format!(
        "{SESSION_COOKIE_NAME}={}; Path=/; Max-Age=43200; HttpOnly; SameSite=Strict{secure_attribute}",
        auth.session_value()
    )
}

fn secure_cookie_for_bind(bind_addr: &str) -> bool {
    bind_addr
        .parse::<SocketAddr>()
        .map(|address| !address.ip().is_loopback())
        .unwrap_or(true)
}

const LOGIN_PAGE: &str = r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Helixflow 登录</title>
  <style>
    html,body{height:100%;margin:0;background:#080b0d;color:#eef7f7;font:16px system-ui,sans-serif}
    body{display:grid;place-items:center}.card{width:min(380px,calc(100% - 48px));padding:28px;border:1px solid #263238;border-radius:16px;background:#101518}
    h1{margin:0 0 8px;font-size:22px}p{color:#9eafb4;line-height:1.5}label{display:grid;gap:8px;margin-top:24px}
    input,button{box-sizing:border-box;width:100%;padding:12px;border-radius:8px;font:inherit}input{border:1px solid #34464c;background:#080b0d;color:#fff}
    button{margin-top:12px;border:0;background:#67e8e0;color:#06201f;font-weight:700;cursor:pointer}
  </style>
</head>
<body><main class="card"><h1>Helixflow</h1><p>输入部署管理员提供的访问 Token。</p><form method="post" action="/api/auth/session"><label>访问 Token<input name="token" type="password" autocomplete="current-password" required></label><button type="submit">进入工作台</button></form></main></body>
</html>"#;

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

    fn header_map(bearer: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(token) = bearer {
            headers.insert(
                axum::http::header::AUTHORIZATION,
                format!("Bearer {token}").parse().expect("header value"),
            );
        }
        headers
    }

    #[test]
    fn query_token_is_rejected_even_on_the_websocket_route() {
        let auth = AuthConfig::with_token(Some("secret"));
        let ws: axum::http::Uri = "/ws?workspace_id=ws_1&token=secret".parse().expect("uri");
        assert!(!request_is_authorized(&auth, &header_map(None), &ws));

        let rest: axum::http::Uri = "/api/workspaces/ws_1/state?token=secret"
            .parse()
            .expect("uri");
        assert!(!request_is_authorized(&auth, &header_map(None), &rest));
    }

    #[test]
    fn derived_http_only_cookie_authorizes_browser_requests() {
        let auth = AuthConfig::with_token(Some("secret"));
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            format!("theme=dark; helixflow_session={}", auth.session_value())
                .parse()
                .expect("cookie header"),
        );
        let uri: axum::http::Uri = "/api/workspaces".parse().expect("uri");

        assert!(request_is_authorized(&auth, &headers, &uri));
        assert!(!auth.session_value().contains("secret"));
    }

    #[test]
    fn browser_session_cookie_is_http_only_strict_and_secret_free() {
        let auth = AuthConfig::with_token(Some("secret"));

        let header = session_cookie_header(&auth, true);

        assert!(header.contains("HttpOnly"));
        assert!(header.contains("SameSite=Strict"));
        assert!(header.contains("Secure"));
        assert!(header.contains("Path=/"));
        assert!(!header.contains("secret"));
    }

    #[test]
    fn only_login_page_and_session_creation_bypass_authentication() {
        use axum::http::Method;

        assert!(is_public_auth_route(&Method::GET, "/login"));
        assert!(is_public_auth_route(&Method::POST, "/api/auth/session"));
        assert!(!is_public_auth_route(&Method::GET, "/api/auth/session"));
        assert!(!is_public_auth_route(&Method::POST, "/api/workspaces"));
    }

    #[test]
    fn bearer_header_is_accepted_on_every_route() {
        let rest: axum::http::Uri = "/api/health".parse().expect("uri");
        assert_eq!(
            presented_token(&header_map(Some("secret")), &rest).as_deref(),
            Some("secret")
        );
    }

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
