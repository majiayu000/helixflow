#[cfg(test)]
use std::collections::HashSet;
use std::future::Future;
#[cfg(test)]
use std::net::IpAddr;
use std::time::Duration;

use helixflow_gateway::ArtifactPayload;
use helixflow_gateway::remote_fetch::{self, RemoteFetchError, RemoteFetchPolicy};
#[cfg(test)]
use reqwest::StatusCode;
use reqwest::Url;
use reqwest::header::HeaderValue;

use crate::artifact_path::PendingArtifact;
use crate::artifacts::validate_artifact_bytes;
use crate::{RunError, RunResult};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_REDIRECTS: usize = 5;
const MAX_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;

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

    fn fetch_policy(self) -> RemoteFetchPolicy {
        RemoteFetchPolicy::new(
            self.connect_timeout,
            self.total_timeout,
            self.max_redirects,
            self.max_bytes,
        )
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
    let fetched = remote_fetch::fetch_remote_bytes(raw_url, policy.fetch_policy())
        .await
        .map_err(map_fetch_error)?;
    let content_type = fetched
        .content_type
        .as_deref()
        .and_then(|value| HeaderValue::from_str(value).ok());
    validate_response_mime(content_type.as_ref(), &payload.mime)?;

    let mut pending = match relative {
        Some(relative) => PendingArtifact::create_at(root, relative).await?,
        None => PendingArtifact::create(root, extension).await?,
    };
    if let Err(error) = pending.write_chunk(&fetched.bytes).await {
        pending.abort().await;
        return Err(error);
    }
    if let Err(error) = validate_artifact_bytes(payload, &fetched.bytes) {
        pending.abort().await;
        return Err(error);
    }
    pending.publish().await
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
    remote_fetch::parse_remote_url(raw_url, false).map_err(map_fetch_error)
}

#[cfg(test)]
pub(crate) fn validate_resolved_addresses(addresses: &[IpAddr]) -> RunResult<()> {
    remote_fetch::validate_resolved_addresses(addresses).map_err(map_fetch_error)
}

#[cfg(test)]
pub(crate) fn validate_resolved_addresses_for_url(
    url: &Url,
    addresses: &[IpAddr],
) -> RunResult<()> {
    remote_fetch::validate_resolved_addresses_for_url(url, addresses).map_err(map_fetch_error)
}

#[cfg(test)]
pub(crate) fn is_public_address(address: IpAddr) -> bool {
    remote_fetch::is_public_address(address)
}

#[cfg(test)]
pub(crate) fn next_redirect_url(
    current: &Url,
    status: StatusCode,
    location: Option<&HeaderValue>,
    visited: &mut HashSet<String>,
    redirect_count: &mut usize,
    max_redirects: usize,
) -> RunResult<Url> {
    let policy = RemoteFetchPolicy::new(
        CONNECT_TIMEOUT,
        TOTAL_TIMEOUT,
        max_redirects,
        MAX_ARTIFACT_BYTES,
    );
    remote_fetch::next_redirect_url(current, status, location, visited, redirect_count, policy)
        .map_err(map_fetch_error)
}

#[cfg(test)]
pub(crate) fn validate_declared_size(content_length: Option<u64>, max: u64) -> RunResult<()> {
    if content_length.is_some_and(|length| length > max) {
        return Err(remote_error("remote artifact declared size exceeds limit"));
    }
    Ok(())
}

#[cfg(test)]
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
    content_type: Option<&HeaderValue>,
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

fn map_fetch_error(error: RemoteFetchError) -> RunError {
    let message = match error {
        RemoteFetchError::InvalidUrl => "remote artifact URL is invalid",
        RemoteFetchError::MustUseHttps => "remote artifact URL must use https",
        RemoteFetchError::CredentialsNotAllowed => {
            "remote artifact URL credentials are not allowed"
        }
        RemoteFetchError::HostInvalid => "remote artifact URL host is invalid",
        RemoteFetchError::TargetNotAllowed => "remote artifact target is not allowed",
        RemoteFetchError::PortInvalid => "remote artifact URL port is invalid",
        RemoteFetchError::DnsFailed => "remote artifact DNS resolution failed",
        RemoteFetchError::DnsEmpty => "remote artifact DNS resolution returned no addresses",
        RemoteFetchError::ClientCreateFailed => "remote artifact client could not be created",
        RemoteFetchError::RequestFailed => "remote artifact request failed",
        RemoteFetchError::HttpStatus(status) => {
            return remote_error(&format!("remote artifact returned HTTP {status}"));
        }
        RemoteFetchError::RedirectLimitExceeded => "remote artifact redirect limit exceeded",
        RemoteFetchError::RedirectLocationInvalid => "remote artifact redirect location is invalid",
        RemoteFetchError::RedirectLoop => "remote artifact redirect loop detected",
        RemoteFetchError::NotARedirect => "remote artifact response is not a redirect",
        RemoteFetchError::DeclaredSizeExceedsLimit => "remote artifact declared size exceeds limit",
        RemoteFetchError::BodyFailed => "remote artifact body failed",
        RemoteFetchError::BodySizeExceedsLimit => "remote artifact body size exceeds limit",
        RemoteFetchError::TotalTimeout => "remote artifact total timeout exceeded",
    };
    remote_error(message)
}

fn remote_error(message: &str) -> RunError {
    RunError::ArtifactPersistence(message.to_owned())
}
