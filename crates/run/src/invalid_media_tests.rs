use async_trait::async_trait;
use helixflow_gateway::{
    ArtifactContent, ArtifactKind, ArtifactPayload, CostEstimate, MockProvider, Provider,
    ProviderCatalog, ProviderError, ProviderHealth, ProviderRequest, ProviderResult,
    ProviderResultValue, ProviderTaskHandle,
};
use serde_json::json;

use crate::artifacts::persist_provider_artifact;
use crate::tests::{executable_graph, open_temp_store, workspace_version};
use crate::{EventBus, ManualRunRequest, RunService};

#[tokio::test]
async fn invalid_media_png_signature_only_is_rejected_before_write() {
    let dir = tempfile::tempdir().expect("temp dir");
    let payload = ArtifactPayload {
        kind: ArtifactKind::Image,
        mime: "image/png".to_owned(),
        storage_uri: String::new(),
        content: ArtifactContent::InlineBytes {
            bytes: vec![137, 80, 78, 71],
            ext_hint: Some("png".to_owned()),
        },
        width: Some(1),
        height: Some(1),
        duration_ms: None,
        meta: json!({}),
    };

    let err = persist_provider_artifact(dir.path(), "run_1", "step_1", "image", &payload)
        .await
        .expect_err("PNG signature without chunks must be rejected");

    assert!(
        err.to_string()
            .contains("invalid image/png artifact content")
    );
    assert!(!dir.path().join("artifacts").exists());
}

#[tokio::test]
async fn invalid_media_text_mp4_is_rejected_before_write() {
    let dir = tempfile::tempdir().expect("temp dir");
    let payload = ArtifactPayload {
        kind: ArtifactKind::Video,
        mime: "video/mp4".to_owned(),
        storage_uri: String::new(),
        content: ArtifactContent::InlineBytes {
            bytes: b"helixflow mock video artifact\n".to_vec(),
            ext_hint: Some("mp4".to_owned()),
        },
        width: Some(16),
        height: Some(16),
        duration_ms: Some(200),
        meta: json!({}),
    };

    let err = persist_provider_artifact(dir.path(), "run_1", "step_1", "video", &payload)
        .await
        .expect_err("text declared as MP4 must be rejected");

    assert!(
        err.to_string()
            .contains("invalid video/mp4 artifact content")
    );
    assert!(!dir.path().join("artifacts").exists());
}

#[tokio::test]
async fn invalid_media_provider_response_fails_step_and_run_without_video_artifact() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let artifact_dir = tempfile::tempdir().expect("artifact dir");
    let events = EventBus::new(32);
    let service = RunService::with_provider_events_and_artifact_root(
        store.clone(),
        InvalidMediaProvider::default(),
        events.clone(),
        artifact_dir.path(),
    );
    let mut receiver = events.subscribe();

    let err = service
        .execute_manual_run(ManualRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Invalid media response".to_owned(),
            provider: "mock".to_owned(),
            graph: executable_graph(),
            force_rerun: false,
        })
        .await
        .expect_err("invalid media must fail the run");

    assert!(
        err.to_string()
            .contains("invalid video/mp4 artifact content")
    );
    let events = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
    let run_id = events
        .iter()
        .find(|event| event.ev == "run.failed")
        .map(|event| event.run_id.clone())
        .expect("run.failed event");
    let run = store.run(&run_id).await.expect("failed run");
    let steps = store.run_steps(&run_id).await.expect("run steps");
    let artifacts = store.run_artifacts(&run_id).await.expect("run artifacts");

    assert_eq!(run.status, "failed");
    assert_eq!(
        steps
            .iter()
            .find(|step| step.node_id == "video")
            .expect("video step")
            .state,
        "failed"
    );
    assert!(!artifacts.iter().any(|artifact| artifact.kind == "video"));
    assert!(!events.iter().any(|event| event.ev == "run.succeeded"));
}

#[derive(Clone, Default)]
struct InvalidMediaProvider {
    inner: MockProvider,
}

#[async_trait]
impl Provider for InvalidMediaProvider {
    fn id(&self) -> &str {
        self.inner.id()
    }

    async fn health(&self) -> ProviderHealth {
        self.inner.health().await
    }

    async fn catalog(&self) -> ProviderResultValue<ProviderCatalog> {
        self.inner.catalog().await
    }

    async fn estimate(&self, req: ProviderRequest) -> ProviderResultValue<CostEstimate> {
        self.inner.estimate(req).await
    }

    async fn invoke(&self, req: ProviderRequest) -> ProviderResultValue<ProviderResult> {
        let is_video = req.capability == "text_to_video";
        let mut result = self.inner.invoke(req).await?;
        if is_video {
            let video = result.outputs.get_mut("video").ok_or_else(|| {
                ProviderError::InvalidResponse("mock video is missing".to_owned())
            })?;
            video.content = ArtifactContent::InlineBytes {
                bytes: b"not an mp4".to_vec(),
                ext_hint: Some("mp4".to_owned()),
            };
        }
        Ok(result)
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        self.inner.cancel(handle).await
    }
}
