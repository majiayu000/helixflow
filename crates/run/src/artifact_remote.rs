use std::collections::HashSet;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use helixflow_gateway::ArtifactPayload;
use reqwest::header::{CONTENT_TYPE, LOCATION};
use reqwest::{StatusCode, Url};

use crate::artifact_path::PendingArtifact;
use crate::artifacts::validate_artifact_bytes;
use crate::{RunError, RunResult};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_REDIRECTS: usize = 5;
const MAX_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;
const ATLAS_TOS_TUNNEL_SUFFIX: &str = ".tos-ap-southeast-1.volces.com";

#[derive(Debug, Clone, Copy)]
pub(crate) struct RemotePolicy {
    pub(crate) connect_timeout: Duration,
    pub(crate) total_timeout: Duration,
    pub(crate) max_redirects: usize,
    pub(crate) max_bytes: u64,
}

impl RemotePolicy {
    pub(crate) fn production() -> Self {
        Self {
            connect_timeout: CONNECT_TIMEOUT,
            total_timeout: TOTAL_TIMEOUT,
            max_redirects: MAX_REDIRECTS,
            max_bytes: MAX_ARTIFACT_BYTES,
        }
    }
}

#[cfg(test)]
pub(crate) async fn download_remote_artifact(
    root: &std::path::Path,
    raw_url: &str,
    payload: &ArtifactPayload,
    extension: &str,
) -> RunResult<std::path::PathBuf> {
    let policy = RemotePolicy::production();
    with_total_timeout(
        policy.total_timeout,
        download_remote_artifact_inner(root, raw_url, payload, extension, None, policy),
    )
    .await
}

pub(crate) async fn download_remote_artifact_to(
    root: &std::path::Path,
    raw_url: &str,
    payload: &ArtifactPayload,
    extension: &str,
    relative: &std::path::Path,
) -> RunResult<std::path::PathBuf> {
    let policy = RemotePolicy::production();
    with_total_timeout(
        policy.total_timeout,
        download_remote_artifact_inner(root, raw_url, payload, extension, Some(relative), policy),
    )
    .await
}

async fn download_remote_artifact_inner(
    root: &std::path::Path,
    raw_url: &str,
    payload: &ArtifactPayload,
    extension: &str,
    relative: Option<&std::path::Path>,
    policy: RemotePolicy,
) -> RunResult<std::path::PathBuf> {
    let mut current = parse_remote_url(raw_url)?;
    let mut visited = HashSet::new();
    visited.insert(current.as_str().to_owned());
    let mut redirect_count = 0usize;

    loop {
        let resolved = resolve_and_validate(&current).await?;
        let client = pinned_client(&current, &resolved, policy)?;
        let mut response = client
            .get(current.clone())
            .send()
            .await
            .map_err(|_| remote_error("remote artifact request failed"))?;

        if response.status().is_redirection() {
            current = next_redirect_url(
                &current,
                response.status(),
                response.headers().get(LOCATION),
                &mut visited,
                &mut redirect_count,
                policy.max_redirects,
            )?;
            continue;
        }
        if !response.status().is_success() {
            return Err(remote_error(&format!(
                "remote artifact returned HTTP {}",
                response.status().as_u16()
            )));
        }

        validate_response_mime(response.headers().get(CONTENT_TYPE), &payload.mime)?;
        validate_declared_size(response.content_length(), policy.max_bytes)?;

        let mut pending = match relative {
            Some(relative) => PendingArtifact::create_at(root, relative).await?,
            None => PendingArtifact::create(root, extension).await?,
        };
        let mut received = 0u64;
        let mut bytes = Vec::with_capacity(
            response
                .content_length()
                .unwrap_or_default()
                .min(policy.max_bytes) as usize,
        );
        loop {
            let chunk = match response.chunk().await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(_) => {
                    pending.abort().await;
                    return Err(remote_error("remote artifact body failed"));
                }
            };
            received = match checked_received_size(received, chunk.len(), policy.max_bytes) {
                Ok(size) => size,
                Err(error) => {
                    pending.abort().await;
                    return Err(error);
                }
            };
            if let Err(error) = pending.write_chunk(&chunk).await {
                pending.abort().await;
                return Err(error);
            }
            bytes.extend_from_slice(&chunk);
        }

        if let Err(error) = validate_artifact_bytes(payload, &bytes) {
            pending.abort().await;
            return Err(error);
        }
        return pending.publish().await;
    }
}

pub(crate) async fn with_total_timeout<T>(
    duration: Duration,
    operation: impl Future<Output = RunResult<T>>,
) -> RunResult<T> {
    tokio::time::timeout(duration, operation)
        .await
        .map_err(|_| remote_error("remote artifact total timeout exceeded"))?
}

pub(crate) fn parse_remote_url(raw_url: &str) -> RunResult<Url> {
    let parsed = Url::parse(raw_url).map_err(|_| remote_error("remote artifact URL is invalid"))?;
    validate_remote_url(&parsed)?;
    Ok(parsed)
}

pub(crate) fn validate_remote_url(url: &Url) -> RunResult<()> {
    if url.scheme() != "https" {
        return Err(remote_error("remote artifact URL must use https"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(remote_error(
            "remote artifact URL credentials are not allowed",
        ));
    }
    let host = url
        .host()
        .ok_or_else(|| remote_error("remote artifact URL host is invalid"))?;
    match host {
        url::Host::Ipv4(address) => validate_resolved_addresses(&[IpAddr::V4(address)]),
        url::Host::Ipv6(address) => validate_resolved_addresses(&[IpAddr::V6(address)]),
        url::Host::Domain(domain) => {
            let normalized = domain.trim_end_matches('.').to_ascii_lowercase();
            let local_only = normalized == "localhost"
                || normalized.ends_with(".localhost")
                || normalized.ends_with(".local")
                || normalized.ends_with(".internal")
                || normalized.ends_with(".home.arpa");
            if local_only {
                return Err(remote_error("remote artifact target is not allowed"));
            }
            Ok(())
        }
    }
}

async fn resolve_and_validate(url: &Url) -> RunResult<Vec<SocketAddr>> {
    validate_remote_url(url)?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| remote_error("remote artifact URL port is invalid"))?;
    let addresses = match url.host() {
        Some(url::Host::Ipv4(address)) => vec![SocketAddr::new(IpAddr::V4(address), port)],
        Some(url::Host::Ipv6(address)) => vec![SocketAddr::new(IpAddr::V6(address), port)],
        Some(url::Host::Domain(domain)) => tokio::net::lookup_host((domain, port))
            .await
            .map_err(|_| remote_error("remote artifact DNS resolution failed"))?
            .collect(),
        None => return Err(remote_error("remote artifact URL host is invalid")),
    };
    if addresses.is_empty() {
        return Err(remote_error(
            "remote artifact DNS resolution returned no addresses",
        ));
    }
    let ips = addresses
        .iter()
        .map(|address| address.ip())
        .collect::<Vec<_>>();
    validate_resolved_addresses_for_url(url, &ips)?;
    Ok(addresses)
}

fn pinned_client(
    url: &Url,
    addresses: &[SocketAddr],
    policy: RemotePolicy,
) -> RunResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(policy.connect_timeout);
    if let Some(url::Host::Domain(domain)) = url.host() {
        builder = builder.resolve_to_addrs(domain, addresses);
    }
    builder
        .build()
        .map_err(|_| remote_error("remote artifact client could not be created"))
}

pub(crate) fn validate_resolved_addresses(addresses: &[IpAddr]) -> RunResult<()> {
    if addresses.is_empty() || addresses.iter().any(|address| !is_public_address(*address)) {
        return Err(remote_error("remote artifact target is not allowed"));
    }
    Ok(())
}

/// Clash-style TUN DNS intentionally maps public names into 198.18.0.0/15.
/// Keep the default SSRF policy fail-closed and permit that synthetic range
/// only for Atlas' exact Volcengine TOS CDN suffix. Literal benchmark IPs,
/// other hostnames, mixed private answers, and redirects remain rejected.
pub(crate) fn validate_resolved_addresses_for_url(
    url: &Url,
    addresses: &[IpAddr],
) -> RunResult<()> {
    if validate_resolved_addresses(addresses).is_ok() {
        return Ok(());
    }
    if addresses.is_empty() || !is_atlas_tos_tunnel_host(url) {
        return Err(remote_error("remote artifact target is not allowed"));
    }
    if addresses.iter().all(|address| match address {
        IpAddr::V4(address) => is_benchmark_tunnel_ipv4(*address),
        IpAddr::V6(_) => false,
    }) {
        return Ok(());
    }
    Err(remote_error("remote artifact target is not allowed"))
}

fn is_atlas_tos_tunnel_host(url: &Url) -> bool {
    let Some(url::Host::Domain(domain)) = url.host() else {
        return false;
    };
    domain
        .trim_end_matches('.')
        .to_ascii_lowercase()
        .ends_with(ATLAS_TOS_TUNNEL_SUFFIX)
}

fn is_benchmark_tunnel_ipv4(address: Ipv4Addr) -> bool {
    let [a, b, _, _] = address.octets();
    a == 198 && (b == 18 || b == 19)
}

pub(crate) fn is_public_address(address: IpAddr) -> bool {
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
    let [a, b, c, _] = address.octets();
    !(a == 0
        || a == 10
        || a == 127
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 88 && c == 99)
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224)
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

pub(crate) fn next_redirect_url(
    current: &Url,
    status: StatusCode,
    location: Option<&reqwest::header::HeaderValue>,
    visited: &mut HashSet<String>,
    redirect_count: &mut usize,
    max_redirects: usize,
) -> RunResult<Url> {
    if !status.is_redirection() {
        return Err(remote_error("remote artifact response is not a redirect"));
    }
    if *redirect_count >= max_redirects {
        return Err(remote_error("remote artifact redirect limit exceeded"));
    }
    let location = location
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| remote_error("remote artifact redirect location is invalid"))?;
    let next = current
        .join(location)
        .map_err(|_| remote_error("remote artifact redirect location is invalid"))?;
    validate_remote_url(&next)?;
    if !visited.insert(next.as_str().to_owned()) {
        return Err(remote_error("remote artifact redirect loop detected"));
    }
    *redirect_count += 1;
    Ok(next)
}

pub(crate) fn validate_declared_size(content_length: Option<u64>, max: u64) -> RunResult<()> {
    if content_length.is_some_and(|length| length > max) {
        return Err(remote_error("remote artifact declared size exceeds limit"));
    }
    Ok(())
}

pub(crate) fn checked_received_size(current: u64, chunk: usize, max: u64) -> RunResult<u64> {
    let chunk = u64::try_from(chunk)
        .map_err(|_| remote_error("remote artifact body size exceeds limit"))?;
    let next = current
        .checked_add(chunk)
        .ok_or_else(|| remote_error("remote artifact body size exceeds limit"))?;
    if next > max {
        return Err(remote_error("remote artifact body size exceeds limit"));
    }
    Ok(next)
}

pub(crate) fn validate_response_mime(
    content_type: Option<&reqwest::header::HeaderValue>,
    expected: &str,
) -> RunResult<()> {
    let expected = expected
        .parse::<mime::Mime>()
        .map_err(|_| remote_error("remote artifact declared MIME is invalid"))?;
    let actual = content_type
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<mime::Mime>().ok())
        .ok_or_else(|| remote_error("remote artifact response MIME is missing or invalid"))?;
    if actual.type_() != expected.type_() || actual.subtype() != expected.subtype() {
        return Err(remote_error(
            "remote artifact response MIME does not match payload",
        ));
    }
    Ok(())
}

fn remote_error(message: &str) -> RunError {
    RunError::ArtifactPersistence(message.to_owned())
}
