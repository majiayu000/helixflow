use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use base64::Engine;
use helixflow_gateway::{ArtifactContent, ArtifactKind, ArtifactPayload};
use reqwest::StatusCode;
use reqwest::header::HeaderValue;

use crate::artifact_path::PendingArtifact;
use crate::artifact_remote::{
    RemotePolicy, checked_received_size, download_remote_artifact, is_public_address,
    next_redirect_url, parse_remote_url, validate_declared_size, validate_resolved_addresses,
    validate_resolved_addresses_for_url, validate_response_mime, with_total_timeout,
};
use crate::artifacts::validate_artifact_bytes;
use crate::{RunError, RunResult};

fn remote_text_payload(url: &str) -> ArtifactPayload {
    ArtifactPayload {
        kind: ArtifactKind::Text,
        mime: "text/plain".to_owned(),
        storage_uri: url.to_owned(),
        content: ArtifactContent::RemoteUrl {
            url: url.to_owned(),
        },
        width: None,
        height: None,
        duration_ms: None,
        meta: serde_json::json!({}),
    }
}

#[test]
fn artifact_remote_rejects_loopback_private_link_local_and_reserved_ipv4() {
    let forbidden = [
        Ipv4Addr::new(0, 0, 0, 0),
        Ipv4Addr::new(10, 0, 0, 1),
        Ipv4Addr::new(100, 64, 0, 1),
        Ipv4Addr::new(127, 0, 0, 1),
        Ipv4Addr::new(169, 254, 1, 1),
        Ipv4Addr::new(172, 16, 0, 1),
        Ipv4Addr::new(192, 0, 0, 1),
        Ipv4Addr::new(192, 0, 2, 1),
        Ipv4Addr::new(192, 168, 1, 1),
        Ipv4Addr::new(198, 18, 0, 1),
        Ipv4Addr::new(198, 51, 100, 1),
        Ipv4Addr::new(203, 0, 113, 1),
        Ipv4Addr::new(224, 0, 0, 1),
        Ipv4Addr::new(255, 255, 255, 255),
    ];
    for address in forbidden {
        assert!(!is_public_address(IpAddr::V4(address)), "{address}");
    }
    assert!(is_public_address(IpAddr::V4(Ipv4Addr::new(
        93, 184, 216, 34
    ))));
}

#[test]
fn artifact_remote_rejects_non_global_and_mapped_private_ipv6() {
    let forbidden = [
        Ipv6Addr::UNSPECIFIED,
        Ipv6Addr::LOCALHOST,
        "fe80::1".parse().unwrap(),
        "fc00::1".parse().unwrap(),
        "2001:db8::1".parse().unwrap(),
        "2002:7f00:1::".parse().unwrap(),
        "3fff::1".parse().unwrap(),
        "::ffff:127.0.0.1".parse().unwrap(),
    ];
    for address in forbidden {
        assert!(!is_public_address(IpAddr::V6(address)), "{address}");
    }
    assert!(is_public_address(IpAddr::V6(
        "2606:4700:4700::1111".parse().unwrap()
    )));
}

#[test]
fn artifact_remote_rejects_mixed_public_and_private_dns_results() {
    let mixed = [
        IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
    ];
    assert!(validate_resolved_addresses(&mixed).is_err());
    assert!(validate_resolved_addresses(&[]).is_err());
}

#[test]
fn artifact_remote_allows_clash_fake_ip_only_for_known_atlas_media_hosts() {
    let fake_ip = [IpAddr::V4(Ipv4Addr::new(198, 18, 0, 5))];
    let atlas_tos = reqwest::Url::parse(
        "https://ark-content-generation-ap-southeast-1.tos-ap-southeast-1.volces.com/video.mp4",
    )
    .unwrap();
    let atlas_oss =
        reqwest::Url::parse("https://atlas-media.oss-us-west-1.aliyuncs.com/generated/image.png")
            .unwrap();
    let lookalike = reqwest::Url::parse(
        "https://ark-content-generation-ap-southeast-1.tos-ap-southeast-1.volces.com.attacker.example/video.mp4",
    )
    .unwrap();
    let oss_lookalike = reqwest::Url::parse(
        "https://atlas-media.oss-us-west-1.aliyuncs.com.attacker.example/image.png",
    )
    .unwrap();
    let arbitrary = reqwest::Url::parse("https://example.com/video.mp4").unwrap();
    let literal = reqwest::Url::parse("https://198.18.0.5/video.mp4").unwrap();

    for allowed in [&atlas_tos, &atlas_oss] {
        assert!(
            validate_resolved_addresses_for_url(allowed, &fake_ip).is_ok(),
            "{allowed}"
        );
    }
    for rejected in [lookalike, oss_lookalike, arbitrary, literal] {
        assert!(
            validate_resolved_addresses_for_url(&rejected, &fake_ip).is_err(),
            "{rejected}"
        );
    }
    let mixed_private = [
        IpAddr::V4(Ipv4Addr::new(198, 18, 0, 5)),
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
    ];
    assert!(validate_resolved_addresses_for_url(&atlas_tos, &mixed_private).is_err());
    assert!(validate_resolved_addresses_for_url(&atlas_oss, &mixed_private).is_err());
}

#[test]
fn artifact_remote_accepts_only_credential_free_public_https_urls() {
    assert!(parse_remote_url("https://93.184.216.34/media.png").is_ok());
    for rejected in [
        "http://93.184.216.34/media.png",
        "https://127.0.0.1/media.png",
        "https://[::1]/media.png",
        "https://user:secret@example.com/media.png",
        "https://service.local/media.png",
        "file:///tmp/media.png",
    ] {
        assert!(parse_remote_url(rejected).is_err(), "{rejected}");
    }
}

#[test]
fn artifact_remote_revalidates_every_redirect_and_rejects_loops() {
    let current = parse_remote_url("https://93.184.216.34/start").unwrap();
    let mut visited = HashSet::from([current.as_str().to_owned()]);
    let mut count = 0;
    let private = HeaderValue::from_static("https://127.0.0.1/internal");
    let error = next_redirect_url(
        &current,
        StatusCode::FOUND,
        Some(&private),
        &mut visited,
        &mut count,
        5,
    )
    .expect_err("redirect target policy must run before the next request");
    assert!(error.to_string().contains("target is not allowed"));

    let relative = HeaderValue::from_static("/next");
    let next = next_redirect_url(
        &current,
        StatusCode::TEMPORARY_REDIRECT,
        Some(&relative),
        &mut visited,
        &mut count,
        5,
    )
    .expect("public relative redirect");
    let back = HeaderValue::from_static("/start");
    assert!(
        next_redirect_url(
            &next,
            StatusCode::FOUND,
            Some(&back),
            &mut visited,
            &mut count,
            5,
        )
        .unwrap_err()
        .to_string()
        .contains("loop")
    );
}

#[test]
fn artifact_remote_enforces_redirect_count_and_location() {
    let current = parse_remote_url("https://93.184.216.34/start").unwrap();
    let mut visited = HashSet::from([current.as_str().to_owned()]);
    let mut count = 5;
    let location = HeaderValue::from_static("/next");
    assert!(
        next_redirect_url(
            &current,
            StatusCode::FOUND,
            Some(&location),
            &mut visited,
            &mut count,
            5,
        )
        .unwrap_err()
        .to_string()
        .contains("limit")
    );
    assert!(
        next_redirect_url(&current, StatusCode::FOUND, None, &mut visited, &mut 0, 5,).is_err()
    );
}

#[test]
fn artifact_remote_enforces_declared_and_actual_byte_limits() {
    assert!(validate_declared_size(Some(64), 64).is_ok());
    assert!(validate_declared_size(None, 64).is_ok());
    assert!(validate_declared_size(Some(65), 64).is_err());
    assert_eq!(checked_received_size(32, 32, 64).unwrap(), 64);
    assert!(checked_received_size(64, 1, 64).is_err());
    assert!(checked_received_size(u64::MAX, 1, u64::MAX).is_err());
}

#[test]
fn artifact_remote_requires_matching_response_mime() {
    let text = HeaderValue::from_static("text/plain; charset=utf-8");
    let json = HeaderValue::from_static("application/json");
    assert!(validate_response_mime(Some(&text), "text/plain").is_ok());
    assert!(validate_response_mime(Some(&json), "text/plain").is_err());
    assert!(validate_response_mime(None, "text/plain").is_err());
}

#[test]
fn artifact_remote_production_policy_has_all_resource_bounds() {
    let policy = RemotePolicy::production();
    assert!(policy.connect_timeout > Duration::ZERO);
    assert!(policy.total_timeout > policy.connect_timeout);
    assert!(policy.max_redirects > 0);
    assert!(policy.max_bytes > 0);
}

#[tokio::test]
async fn artifact_remote_total_timeout_is_enforced() {
    let operation = async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        Ok::<_, RunError>(())
    };
    let error = with_total_timeout(Duration::from_millis(1), operation)
        .await
        .expect_err("total timeout must cancel the operation");
    assert!(error.to_string().contains("total timeout"));
}

#[tokio::test]
async fn artifact_remote_actual_limit_failure_cleans_partial_file() {
    let root = tempfile::tempdir().expect("root tempdir");
    let mut pending = PendingArtifact::create(root.path(), "txt")
        .await
        .expect("create partial");
    pending
        .write_chunk(b"1234")
        .await
        .expect("write first chunk");
    let result = checked_received_size(4, 1, 4);
    assert!(result.is_err());
    pending.abort().await;

    assert_eq!(
        std::fs::read_dir(root.path().join("artifacts"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn artifact_remote_sensitive_target_error_is_redacted_before_network() {
    let root = tempfile::tempdir().expect("root tempdir");
    let sensitive_url = "https://user:secret@127.0.0.1:9/private?token=hidden";
    let payload = remote_text_payload(sensitive_url);
    let error = download_remote_artifact(root.path(), sensitive_url, &payload, "txt")
        .await
        .expect_err("sensitive loopback target must be rejected");
    let message = error.to_string();

    assert!(!message.contains("secret"));
    assert!(!message.contains("token=hidden"));
    assert!(!message.contains("127.0.0.1"));
    assert!(!message.contains(root.path().to_string_lossy().as_ref()));
}

#[tokio::test]
async fn artifact_remote_total_timeout_drops_and_cleans_pending_file() {
    let root = tempfile::tempdir().expect("root tempdir");
    let mut pending = PendingArtifact::create(root.path(), "txt")
        .await
        .expect("create partial before timeout");
    let operation = async move {
        pending.write_chunk(b"partial").await?;
        std::future::pending::<RunResult<()>>().await
    };
    assert!(
        with_total_timeout(Duration::from_millis(5), operation)
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read_dir(root.path().join("artifacts"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn artifact_remote_valid_public_https_png_reaches_atomic_publish() {
    const PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAACXBIWXMAAAABAAAAAQBPJcTWAAAADElEQVR4nGNkYGAAAAAIAAI76MGHAAAAAElFTkSuQmCC";
    let root = tempfile::tempdir().expect("root tempdir");
    let url = "https://93.184.216.34/media.png";
    let payload = ArtifactPayload {
        kind: ArtifactKind::Image,
        mime: "image/png".to_owned(),
        storage_uri: url.to_owned(),
        content: ArtifactContent::RemoteUrl {
            url: url.to_owned(),
        },
        width: Some(1),
        height: Some(1),
        duration_ms: None,
        meta: serde_json::json!({}),
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(PNG_BASE64)
        .expect("decode deterministic PNG fixture");
    let content_type = HeaderValue::from_static("image/png");

    assert!(parse_remote_url(url).is_ok());
    validate_response_mime(Some(&content_type), &payload.mime).expect("matching response MIME");
    validate_declared_size(Some(bytes.len() as u64), bytes.len() as u64)
        .expect("bounded content length");
    assert_eq!(
        checked_received_size(0, bytes.len(), bytes.len() as u64).expect("bounded actual bytes"),
        bytes.len() as u64
    );
    validate_artifact_bytes(&payload, &bytes).expect("PR117 PNG validation");
    let mut pending = PendingArtifact::create(root.path(), "png")
        .await
        .expect("create partial");
    pending.write_chunk(&bytes).await.expect("stream PNG chunk");
    let relative = pending.publish().await.expect("atomic publish");

    assert_eq!(std::fs::read(root.path().join(relative)).unwrap(), bytes);
    assert_eq!(
        std::fs::read_dir(root.path().join("artifacts"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn artifact_remote_media_error_does_not_echo_mime_parameters() {
    let payload = ArtifactPayload {
        kind: ArtifactKind::Video,
        mime: "image/png; token=hidden".to_owned(),
        storage_uri: String::new(),
        content: ArtifactContent::InlineBytes {
            bytes: Vec::new(),
            ext_hint: None,
        },
        width: None,
        height: None,
        duration_ms: None,
        meta: serde_json::json!({}),
    };
    let error = validate_artifact_bytes(&payload, &[]).expect_err("kind and MIME must disagree");
    let message = error.to_string();
    assert!(message.contains("image/png"));
    assert!(!message.contains("token=hidden"));
}
