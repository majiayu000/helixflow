use std::path::{Component, Path};

use helixflow_gateway::{ArtifactContent, ArtifactKind, ArtifactPayload};

use crate::artifact_path::{PendingArtifact, validate_relative_path};
use crate::artifacts::persist_provider_artifact;

fn inline_text_payload() -> ArtifactPayload {
    ArtifactPayload {
        kind: ArtifactKind::Text,
        mime: "text/plain".to_owned(),
        storage_uri: String::new(),
        content: ArtifactContent::InlineBytes {
            bytes: b"safe artifact".to_vec(),
            ext_hint: Some("txt".to_owned()),
        },
        width: None,
        height: None,
        duration_ms: None,
        meta: serde_json::json!({}),
    }
}

#[tokio::test]
async fn artifact_path_uses_an_opaque_single_component_filename() {
    let root = tempfile::tempdir().expect("root tempdir");
    let relative = persist_provider_artifact(
        root.path(),
        "../client-run",
        "/absolute/step",
        "..\\nested/secret-node",
        &inline_text_payload(),
    )
    .await
    .expect("persist safe artifact");
    let relative = Path::new(&relative);
    let components = relative.components().collect::<Vec<_>>();

    assert_eq!(components.len(), 2, "expected artifacts/<opaque filename>");
    assert_eq!(components[0], Component::Normal("artifacts".as_ref()));
    let filename = relative
        .file_name()
        .and_then(|value| value.to_str())
        .expect("artifact filename must be UTF-8");
    assert!(!filename.contains("client-run"));
    assert!(!filename.contains("absolute"));
    assert!(!filename.contains("secret-node"));
    assert!(!filename.contains('/') && !filename.contains('\\'));
    assert_eq!(
        std::fs::read(root.path().join(relative)).unwrap(),
        b"safe artifact"
    );
}

#[test]
fn artifact_path_rejects_absolute_parent_and_platform_separator_components() {
    for invalid in [
        Path::new("/absolute/artifact.txt"),
        Path::new("artifacts/../outside.txt"),
        Path::new("artifacts\\outside.txt"),
        Path::new("C:\\outside.txt"),
    ] {
        assert!(
            validate_relative_path(invalid).is_err(),
            "unexpected safe path: {invalid:?}"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn artifact_path_rejects_a_parent_symlink_outside_root() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("root tempdir");
    let outside = tempfile::tempdir().expect("outside tempdir");
    let canary = outside.path().join("canary");
    std::fs::write(&canary, b"unchanged").expect("write canary");
    symlink(outside.path(), root.path().join("artifacts")).expect("create parent symlink");

    let error =
        persist_provider_artifact(root.path(), "run", "step", "node", &inline_text_payload())
            .await
            .expect_err("a root-external parent symlink must fail closed");

    assert!(error.to_string().contains("escapes the configured root"));
    assert_eq!(std::fs::read(canary).unwrap(), b"unchanged");
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn artifact_path_publish_never_overwrites_an_existing_destination() {
    let root = tempfile::tempdir().expect("root tempdir");
    let mut pending = PendingArtifact::create(root.path(), "txt")
        .await
        .expect("create partial");
    pending.write_chunk(b"new").await.expect("write partial");
    let final_path = root.path().join(pending.relative_path());
    tokio::fs::write(&final_path, b"existing")
        .await
        .expect("create collision");

    let error = pending
        .publish()
        .await
        .expect_err("publish must not replace existing file");

    assert!(error.to_string().contains("could not be published"));
    assert_eq!(tokio::fs::read(&final_path).await.unwrap(), b"existing");
    let entries = std::fs::read_dir(root.path().join("artifacts"))
        .unwrap()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 1, "partial file must be cleaned");
}

#[tokio::test]
async fn artifact_path_abort_removes_partial_file() {
    let root = tempfile::tempdir().expect("root tempdir");
    let mut pending = PendingArtifact::create(root.path(), "txt")
        .await
        .expect("create partial");
    pending
        .write_chunk(b"partial")
        .await
        .expect("write partial");
    pending.abort().await;

    assert_eq!(
        std::fs::read_dir(root.path().join("artifacts"))
            .unwrap()
            .count(),
        0
    );
}
