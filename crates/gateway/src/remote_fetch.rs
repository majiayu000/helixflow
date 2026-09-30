//! SSRF-checked remote byte fetch.
//!
//! Callers that must not trust a provider-supplied URL use this instead of a
//! default HTTP client. Redirects are not followed by the client: each hop is
//! checked again, DNS answers are pinned, and proxy environment variables are
//! ignored. `198.18.0.0/15` stays blocked except for known Atlas media hosts.

use std::collections::HashSet;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use reqwest::header::{CONTENT_TYPE, LOCATION};
use reqwest::{StatusCode, Url};
use url::Host;

const ATLAS_TOS_TUNNEL_SUFFIX: &str = ".tos-ap-southeast-1.volces.com";
const ATLAS_OSS_TUNNEL_HOST: &str = "atlas-media.oss-us-west-1.aliyuncs.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteFetchPolicy {
    connect_timeout: Duration,
    total_timeout: Duration,
    max_redirects: usize,
    max_bytes: u64,
    allow_loopback_http: bool,
}

impl RemoteFetchPolicy {
    pub fn new(
        connect_timeout: Duration,
        total_timeout: Duration,
        max_redirects: usize,
        max_bytes: u64,
    ) -> Self {
        Self {
            connect_timeout,
            total_timeout,
            max_redirects,
            max_bytes,
            allow_loopback_http: false,
        }
    }

    /// Permit `http` only for literal loopback IPs and the exact host `localhost`.
    ///
    /// HTTPS loopback, private, and link-local targets stay rejected.
    pub fn allow_loopback_http(mut self, allow: bool) -> Self {
        self.allow_loopback_http = allow;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFetchBody {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
    pub content_length: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteFetchError {
    InvalidUrl,
    MustUseHttps,
    CredentialsNotAllowed,
    HostInvalid,
    TargetNotAllowed,
    PortInvalid,
    DnsFailed,
    DnsEmpty,
    ClientCreateFailed,
    RequestFailed,
    HttpStatus(u16),
    RedirectLimitExceeded,
    RedirectLocationInvalid,
    RedirectLoop,
    NotARedirect,
    DeclaredSizeExceedsLimit,
    BodyFailed,
    BodySizeExceedsLimit,
    TotalTimeout,
}

impl RemoteFetchError {
    fn message(self) -> &'static str {
        match self {
            Self::InvalidUrl => "remote fetch URL is invalid",
            Self::MustUseHttps => "remote fetch URL must use https",
            Self::CredentialsNotAllowed => "remote fetch URL credentials are not allowed",
            Self::HostInvalid => "remote fetch URL host is invalid",
            Self::TargetNotAllowed => "remote fetch target is not allowed",
            Self::PortInvalid => "remote fetch URL port is invalid",
            Self::DnsFailed => "remote fetch DNS resolution failed",
            Self::DnsEmpty => "remote fetch DNS resolution returned no addresses",
            Self::ClientCreateFailed => "remote fetch client could not be created",
            Self::RequestFailed => "remote fetch request failed",
            Self::HttpStatus(_) => "remote fetch returned an HTTP error",
            Self::RedirectLimitExceeded => "remote fetch redirect limit exceeded",
            Self::RedirectLocationInvalid => "remote fetch redirect location is invalid",
            Self::RedirectLoop => "remote fetch redirect loop detected",
            Self::NotARedirect => "remote fetch response is not a redirect",
            Self::DeclaredSizeExceedsLimit => "remote fetch declared size exceeds limit",
            Self::BodyFailed => "remote fetch body failed",
            Self::BodySizeExceedsLimit => "remote fetch body size exceeds limit",
            Self::TotalTimeout => "remote fetch total timeout exceeded",
        }
    }
}

impl fmt::Display for RemoteFetchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for RemoteFetchError {}

pub async fn fetch_remote_bytes(
    raw_url: &str,
    policy: RemoteFetchPolicy,
) -> Result<RemoteFetchBody, RemoteFetchError> {
    let total_timeout = policy.total_timeout;
    match tokio::time::timeout(total_timeout, fetch_remote_bytes_inner(raw_url, policy)).await {
        Ok(result) => result,
        Err(_) => Err(RemoteFetchError::TotalTimeout),
    }
}

async fn fetch_remote_bytes_inner(
    raw_url: &str,
    policy: RemoteFetchPolicy,
) -> Result<RemoteFetchBody, RemoteFetchError> {
    let mut current = parse_remote_url(raw_url, policy.allow_loopback_http)?;
    let mut visited = HashSet::new();
    visited.insert(current.as_str().to_owned());
    let mut redirect_count = 0usize;

    loop {
        let resolved = resolve_and_validate(&current, policy.allow_loopback_http).await?;
        let client = pinned_client(&current, &resolved, policy)?;
        // Client errors include the request URL. Keep the caller-facing error static.
        let mut response = client
            .get(current.clone())
            .send()
            .await
            .map_err(|_| RemoteFetchError::RequestFailed)?;

        if response.status().is_redirection() {
            current = next_redirect_url(
                &current,
                response.status(),
                response.headers().get(LOCATION),
                &mut visited,
                &mut redirect_count,
                policy,
            )?;
            continue;
        }
        if !response.status().is_success() {
            return Err(RemoteFetchError::HttpStatus(response.status().as_u16()));
        }

        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let content_length = response.content_length();
        if content_length.is_some_and(|length| length > policy.max_bytes) {
            return Err(RemoteFetchError::DeclaredSizeExceedsLimit);
        }

        let mut bytes = Vec::new();
        let mut received = 0u64;
        loop {
            let chunk = match response.chunk().await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(_) => return Err(RemoteFetchError::BodyFailed),
            };
            received = checked_received_size(received, chunk.len(), policy.max_bytes)?;
            bytes.extend_from_slice(&chunk);
        }
        return Ok(RemoteFetchBody {
            bytes,
            content_type,
            content_length,
        });
    }
}

pub fn parse_remote_url(raw_url: &str, allow_loopback_http: bool) -> Result<Url, RemoteFetchError> {
    let parsed = Url::parse(raw_url).map_err(|_| RemoteFetchError::InvalidUrl)?;
    validate_remote_url(&parsed, allow_loopback_http)?;
    Ok(parsed)
}

pub fn validate_remote_url(url: &Url, allow_loopback_http: bool) -> Result<(), RemoteFetchError> {
    let loopback_http =
        allow_loopback_http && url.scheme() == "http" && host_is_loopback_exception(url);
    if !loopback_http && url.scheme() != "https" {
        return Err(RemoteFetchError::MustUseHttps);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(RemoteFetchError::CredentialsNotAllowed);
    }
    if loopback_http {
        return Ok(());
    }
    let host = url.host().ok_or(RemoteFetchError::HostInvalid)?;
    match host {
        Host::Ipv4(address) => validate_resolved_addresses(&[IpAddr::V4(address)]),
        Host::Ipv6(address) => validate_resolved_addresses(&[IpAddr::V6(address)]),
        Host::Domain(domain) => {
            if is_local_only_domain(domain) {
                return Err(RemoteFetchError::TargetNotAllowed);
            }
            Ok(())
        }
    }
}

async fn resolve_and_validate(
    url: &Url,
    allow_loopback_http: bool,
) -> Result<Vec<SocketAddr>, RemoteFetchError> {
    validate_remote_url(url, allow_loopback_http)?;
    let port = url
        .port_or_known_default()
        .ok_or(RemoteFetchError::PortInvalid)?;
    let addresses = match url.host() {
        Some(Host::Ipv4(address)) => vec![SocketAddr::new(IpAddr::V4(address), port)],
        Some(Host::Ipv6(address)) => vec![SocketAddr::new(IpAddr::V6(address), port)],
        Some(Host::Domain(domain)) => tokio::net::lookup_host((domain, port))
            .await
            .map_err(|_| RemoteFetchError::DnsFailed)?
            .collect(),
        None => return Err(RemoteFetchError::HostInvalid),
    };
    if addresses.is_empty() {
        return Err(RemoteFetchError::DnsEmpty);
    }
    let ips = addresses
        .iter()
        .map(|address| address.ip())
        .collect::<Vec<_>>();
    validate_resolved_for_policy(url, &ips, allow_loopback_http)?;
    Ok(addresses)
}

fn pinned_client(
    url: &Url,
    addresses: &[SocketAddr],
    policy: RemoteFetchPolicy,
) -> Result<reqwest::Client, RemoteFetchError> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(policy.connect_timeout);
    if let Some(Host::Domain(domain)) = url.host() {
        builder = builder.resolve_to_addrs(domain, addresses);
    }
    builder
        .build()
        .map_err(|_| RemoteFetchError::ClientCreateFailed)
}

pub fn validate_resolved_addresses(addresses: &[IpAddr]) -> Result<(), RemoteFetchError> {
    if addresses.is_empty() || addresses.iter().any(|address| !is_public_address(*address)) {
        return Err(RemoteFetchError::TargetNotAllowed);
    }
    Ok(())
}

/// Clash-style TUN DNS intentionally maps public names into 198.18.0.0/15.
/// Keep the default SSRF policy fail-closed and permit that synthetic range
/// only for Atlas' known media hosts. Literal benchmark IPs, other hostnames,
/// mixed private answers, and redirects remain rejected.
pub fn validate_resolved_addresses_for_url(
    url: &Url,
    addresses: &[IpAddr],
) -> Result<(), RemoteFetchError> {
    if validate_resolved_addresses(addresses).is_ok() {
        return Ok(());
    }
    if addresses.is_empty() || !is_atlas_media_tunnel_host(url) {
        return Err(RemoteFetchError::TargetNotAllowed);
    }
    if addresses.iter().all(|address| match address {
        IpAddr::V4(address) => is_benchmark_tunnel_ipv4(*address),
        IpAddr::V6(_) => false,
    }) {
        return Ok(());
    }
    Err(RemoteFetchError::TargetNotAllowed)
}

fn validate_resolved_for_policy(
    url: &Url,
    addresses: &[IpAddr],
    allow_loopback_http: bool,
) -> Result<(), RemoteFetchError> {
    if allow_loopback_http && url.scheme() == "http" && host_is_loopback_exception(url) {
        if !addresses.is_empty() && addresses.iter().all(|address| address.is_loopback()) {
            return Ok(());
        }
        return Err(RemoteFetchError::TargetNotAllowed);
    }
    validate_resolved_addresses_for_url(url, addresses)
}

fn host_is_loopback_exception(url: &Url) -> bool {
    match url.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        Some(Host::Domain(domain)) => normalized_host(domain) == "localhost",
        None => false,
    }
}

fn is_local_only_domain(domain: &str) -> bool {
    let normalized = normalized_host(domain);
    normalized == "localhost"
        || normalized.ends_with(".localhost")
        || normalized.ends_with(".local")
        || normalized.ends_with(".internal")
        || normalized.ends_with(".home.arpa")
}

fn normalized_host(domain: &str) -> String {
    domain.trim_end_matches('.').to_ascii_lowercase()
}

fn is_atlas_media_tunnel_host(url: &Url) -> bool {
    let Some(Host::Domain(domain)) = url.host() else {
        return false;
    };
    let normalized = normalized_host(domain);
    normalized.ends_with(ATLAS_TOS_TUNNEL_SUFFIX) || normalized == ATLAS_OSS_TUNNEL_HOST
}

fn is_benchmark_tunnel_ipv4(address: Ipv4Addr) -> bool {
    let [first, second, _, _] = address.octets();
    first == 198 && (second == 18 || second == 19)
}

pub fn is_public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_ipv4(address),
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                return is_public_ipv4(mapped);
            }
            is_public_ipv6(address)
        }
    }
}

fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let [first, second, third, _] = address.octets();
    !(first == 0
        || first == 10
        || first == 127
        || (first == 100 && (64..=127).contains(&second))
        || (first == 169 && second == 254)
        || (first == 172 && (16..=31).contains(&second))
        || (first == 192 && second == 0 && third == 0)
        || (first == 192 && second == 0 && third == 2)
        || (first == 192 && second == 88 && third == 99)
        || (first == 192 && second == 168)
        || (first == 198 && (second == 18 || second == 19))
        || (first == 198 && second == 51 && third == 100)
        || (first == 203 && second == 0 && third == 113)
        || first >= 224)
}

fn is_public_ipv6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    let global_unicast = segments[0] & 0xe000 == 0x2000;
    let documentation = segments[0] == 0x2001 && segments[1] == 0x0db8;
    let ietf_special = segments[0] == 0x2001 && segments[1] < 0x0200;
    let six_to_four = segments[0] == 0x2002;
    let extended_documentation = segments[0] == 0x3fff;
    global_unicast && !documentation && !ietf_special && !six_to_four && !extended_documentation
}

pub fn next_redirect_url(
    current: &Url,
    status: StatusCode,
    location: Option<&reqwest::header::HeaderValue>,
    visited: &mut HashSet<String>,
    redirect_count: &mut usize,
    policy: RemoteFetchPolicy,
) -> Result<Url, RemoteFetchError> {
    if !status.is_redirection() {
        return Err(RemoteFetchError::NotARedirect);
    }
    if *redirect_count >= policy.max_redirects {
        return Err(RemoteFetchError::RedirectLimitExceeded);
    }
    let location = location
        .and_then(|value| value.to_str().ok())
        .ok_or(RemoteFetchError::RedirectLocationInvalid)?;
    let next = current
        .join(location)
        .map_err(|_| RemoteFetchError::RedirectLocationInvalid)?;
    validate_remote_url(&next, policy.allow_loopback_http)?;
    if !visited.insert(next.as_str().to_owned()) {
        return Err(RemoteFetchError::RedirectLoop);
    }
    *redirect_count += 1;
    Ok(next)
}

fn checked_received_size(current: u64, chunk: usize, max: u64) -> Result<u64, RemoteFetchError> {
    let chunk = u64::try_from(chunk).map_err(|_| RemoteFetchError::BodySizeExceedsLimit)?;
    let next = current
        .checked_add(chunk)
        .ok_or(RemoteFetchError::BodySizeExceedsLimit)?;
    if next > max {
        return Err(RemoteFetchError::BodySizeExceedsLimit);
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use std::net::Ipv6Addr;

    use reqwest::header::HeaderValue;

    use super::*;

    fn policy(allow_loopback_http: bool) -> RemoteFetchPolicy {
        RemoteFetchPolicy::new(
            Duration::from_secs(10),
            Duration::from_secs(90),
            5,
            32 * 1024 * 1024,
        )
        .allow_loopback_http(allow_loopback_http)
    }

    #[test]
    fn public_https_domain_is_accepted_before_name_resolution() {
        let parsed = parse_remote_url("https://example.com/out.png", false).expect("parse");
        assert_eq!(parsed.host_str(), Some("example.com"));
        assert!(parse_remote_url("https://example.com/out.png", true).is_ok());
    }

    #[test]
    fn loopback_http_exception_is_exact() {
        assert!(parse_remote_url("http://127.0.0.1:9000/out.png", false).is_err());
        assert!(parse_remote_url("http://127.0.0.1:9000/out.png", true).is_ok());
        assert!(parse_remote_url("http://[::1]/out.png", true).is_ok());
        assert!(parse_remote_url("http://localhost/out.png", true).is_ok());
        assert!(parse_remote_url("http://localhost./out.png", true).is_ok());
        for rejected in [
            "https://127.0.0.1/out.png",
            "https://[::1]/out.png",
            "https://localhost/out.png",
            "http://10.0.0.1/out.png",
            "http://api.localhost/out.png",
            "http://localhost.localdomain/out.png",
            "http://user:secret@127.0.0.1/out.png",
        ] {
            assert!(parse_remote_url(rejected, true).is_err(), "{rejected}");
        }
    }

    #[test]
    fn local_special_use_names_are_rejected_before_dns() {
        for name in [
            "localhost",
            "app.localhost",
            "printer.local",
            "db.internal",
            "nas.home.arpa",
        ] {
            let url = format!("https://{name}/out.png");
            assert!(parse_remote_url(&url, false).is_err(), "{url}");
            assert!(parse_remote_url(&url, true).is_err(), "{url}");
        }
    }

    #[test]
    fn localhost_http_requires_every_answer_to_be_loopback() {
        let url = parse_remote_url("http://localhost/out.png", true).expect("localhost");
        let loopback = [IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))];
        assert!(validate_resolved_for_policy(&url, &loopback, true).is_ok());
        let mixed = [
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
        ];
        assert!(validate_resolved_for_policy(&url, &mixed, true).is_err());
        assert!(validate_resolved_for_policy(&url, &[], true).is_err());
        assert!(validate_resolved_for_policy(&url, &loopback, false).is_err());
        let mapped = [IpAddr::V6("::ffff:127.0.0.1".parse::<Ipv6Addr>().unwrap())];
        assert!(validate_resolved_for_policy(&url, &mapped, true).is_err());
    }

    #[test]
    fn opted_in_redirects_are_revalidated_without_echoing_the_target() {
        let current = parse_remote_url("http://127.0.0.1/start", true).expect("source");
        let mut visited = HashSet::from([current.as_str().to_owned()]);
        let mut count = 0usize;
        let allowed = HeaderValue::from_static("https://93.184.216.34/next");
        assert!(
            next_redirect_url(
                &current,
                StatusCode::FOUND,
                Some(&allowed),
                &mut visited,
                &mut count,
                policy(true),
            )
            .is_ok()
        );

        for location in [
            "https://127.0.0.1/secret",
            "https://169.254.169.254/latest/meta-data",
            "https://10.0.0.1/secret",
        ] {
            let mut visited = HashSet::from([current.as_str().to_owned()]);
            let mut count = 0usize;
            let header = HeaderValue::from_str(location).expect("location");
            let error = next_redirect_url(
                &current,
                StatusCode::FOUND,
                Some(&header),
                &mut visited,
                &mut count,
                policy(true),
            )
            .expect_err(location);
            assert_eq!(error, RemoteFetchError::TargetNotAllowed);
            let rendered = error.to_string();
            assert!(!rendered.contains("127.0.0.1"), "{rendered}");
            assert!(!rendered.contains("169.254.169.254"), "{rendered}");
            assert!(!rendered.contains("10.0.0.1"), "{rendered}");
            assert!(!rendered.contains("secret"), "{rendered}");
            assert!(!rendered.contains("meta-data"), "{rendered}");
        }
    }

    #[test]
    fn benchmark_tunnel_is_limited_to_known_atlas_media_hosts() {
        let fake_ip = [IpAddr::V4(Ipv4Addr::new(198, 18, 0, 5))];
        let atlas_tos = Url::parse(
            "https://ark-content-generation-ap-southeast-1.tos-ap-southeast-1.volces.com/video.mp4",
        )
        .unwrap();
        let atlas_oss =
            Url::parse("https://atlas-media.oss-us-west-1.aliyuncs.com/generated/image.png")
                .unwrap();
        let lookalike = Url::parse(
            "https://ark-content-generation-ap-southeast-1.tos-ap-southeast-1.volces.com.attacker.example/video.mp4",
        )
        .unwrap();
        let literal = Url::parse("https://198.18.0.5/video.mp4").unwrap();
        assert!(validate_resolved_addresses_for_url(&atlas_tos, &fake_ip).is_ok());
        assert!(validate_resolved_addresses_for_url(&atlas_oss, &fake_ip).is_ok());
        assert!(validate_resolved_addresses_for_url(&lookalike, &fake_ip).is_err());
        assert!(validate_resolved_addresses_for_url(&literal, &fake_ip).is_err());
        assert!(validate_resolved_addresses(&[]).is_err());
        assert!(
            validate_resolved_addresses(&[
                IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            ])
            .is_err()
        );
    }

    #[tokio::test]
    async fn opted_in_loopback_redirect_fails_before_the_next_connection() {
        let blocked = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let blocked_address = blocked.local_addr().unwrap();
        let targets = [
            (format!("https://{blocked_address}/secret"), Some(blocked)),
            ("https://169.254.169.254/latest/meta-data".to_owned(), None),
            ("https://10.0.0.1/secret".to_owned(), None),
        ];
        for (target, blocked) in targets {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let location = target.clone();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 2048];
                let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await;
                let response = format!(
                    "HTTP/1.1 302 Found\r\nlocation: {location}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                );
                tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes())
                    .await
                    .unwrap();
            });
            let result = tokio::time::timeout(
                Duration::from_secs(2),
                fetch_remote_bytes(&format!("http://{address}/out.png"), policy(true)),
            )
            .await
            .expect("policy rejection must not wait on the redirect target");
            server.abort();
            assert_eq!(result, Err(RemoteFetchError::TargetNotAllowed), "{target}");
            let rendered = result.unwrap_err().to_string();
            assert!(!rendered.contains("169.254.169.254"), "{rendered}");
            assert!(!rendered.contains("10.0.0.1"), "{rendered}");
            assert!(!rendered.contains("secret"), "{rendered}");
            assert!(!rendered.contains("meta-data"), "{rendered}");
            if let Some(blocked) = blocked {
                assert!(
                    tokio::time::timeout(Duration::from_millis(200), blocked.accept())
                        .await
                        .is_err(),
                    "redirect target accepted a connection"
                );
            }
        }
    }
}
