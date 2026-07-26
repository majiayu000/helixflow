use std::collections::BTreeMap;

use helixflow_gateway::MockProvider;
use helixflow_graph::{GraphNode, WorkflowGraph};
use serde_json::json;

use super::{EventBus, ManualRunRequest, RunService};
use crate::tests::{open_temp_store, workspace_version};

fn valid_png_bytes() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, //
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, //
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15,
        0xC4, 0x89, //
        0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, //
        0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, //
        0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

fn upload_image_graph(storage_uri: String) -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([(
            "photo".to_owned(),
            GraphNode {
                node_type: "input.image".to_owned(),
                title: "Photo".to_owned(),
                params: json!({ "storage_uri": storage_uri }),
                pos: [0.0, 0.0],
                size: None,
            },
        )]),
        edges: Vec::new(),
    }
}

#[tokio::test]
async fn input_image_resolves_upload_uri_to_persisted_file() {
    let (store, dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let upload_relative = format!("uploads/{workspace_id}/pic.png");
    let upload_path = dir.path().join(&upload_relative);
    tokio::fs::create_dir_all(upload_path.parent().expect("upload parent"))
        .await
        .expect("create upload dir");
    tokio::fs::write(&upload_path, valid_png_bytes())
        .await
        .expect("write upload file");
    let upload = store
        .create_upload(helixflow_store::NewUpload {
            workspace_id: &workspace_id,
            filename: "pic.png",
            file_path: &upload_relative,
            sha256: "sha256:test",
            mime: Some("image/png"),
        })
        .await
        .expect("create upload");

    let service = RunService::with_provider_events_and_artifact_root(
        store.clone(),
        MockProvider::new(),
        EventBus::new(16),
        dir.path().to_path_buf(),
    );

    let outcome = service
        .execute_manual_run(ManualRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Upload input".to_owned(),
            provider: "mock".to_owned(),
            graph: upload_image_graph(format!("upload://{}", upload.id)),
            force_rerun: false,
        })
        .await
        .expect("execute run");

    assert_eq!(outcome.run.status, "succeeded");
    let image = outcome
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == "image")
        .expect("image artifact");
    assert_eq!(image.storage_uri, upload_relative);
    assert_eq!(image.sha256.as_deref(), Some("sha256:test"));
    assert_eq!(image.mime.as_deref(), Some("image/png"));
}

#[tokio::test]
async fn input_image_rejects_upload_from_another_workspace() {
    let (store, dir) = open_temp_store().await;
    let (owner_workspace_id, _) = workspace_version(&store).await;
    let (other_workspace_id, other_version_id) = workspace_version(&store).await;
    let upload_relative = format!("uploads/{owner_workspace_id}/secret.png");
    let upload_path = dir.path().join(&upload_relative);
    tokio::fs::create_dir_all(upload_path.parent().expect("upload parent"))
        .await
        .expect("create upload dir");
    tokio::fs::write(&upload_path, valid_png_bytes())
        .await
        .expect("write upload file");
    let upload = store
        .create_upload(helixflow_store::NewUpload {
            workspace_id: &owner_workspace_id,
            filename: "secret.png",
            file_path: &upload_relative,
            sha256: "sha256:secret",
            mime: Some("image/png"),
        })
        .await
        .expect("create upload");

    let service = RunService::with_provider_events_and_artifact_root(
        store.clone(),
        MockProvider::new(),
        EventBus::new(16),
        dir.path().to_path_buf(),
    );

    let outcome = service
        .execute_manual_run(ManualRunRequest {
            workspace_id: other_workspace_id,
            version_id: other_version_id,
            group_id: None,
            label: "Cross-workspace upload".to_owned(),
            provider: "mock".to_owned(),
            graph: upload_image_graph(format!("upload://{}", upload.id)),
            force_rerun: false,
        })
        .await;
    assert!(
        outcome.is_err(),
        "a run must not resolve another workspace's upload"
    );
}

#[tokio::test]
async fn input_image_with_missing_upload_file_fails_run() {
    let (store, dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let upload = store
        .create_upload(helixflow_store::NewUpload {
            workspace_id: &workspace_id,
            filename: "gone.png",
            file_path: "uploads/nowhere/gone.png",
            sha256: "sha256:test",
            mime: Some("image/png"),
        })
        .await
        .expect("create upload");

    let service = RunService::with_provider_events_and_artifact_root(
        store.clone(),
        MockProvider::new(),
        EventBus::new(16),
        dir.path().to_path_buf(),
    );

    let outcome = service
        .execute_manual_run(ManualRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Missing upload".to_owned(),
            provider: "mock".to_owned(),
            graph: upload_image_graph(format!("upload://{}", upload.id)),
            force_rerun: false,
        })
        .await;
    assert!(outcome.is_err(), "missing upload file must fail the run");
}
