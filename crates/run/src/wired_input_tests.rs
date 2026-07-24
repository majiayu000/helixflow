use std::sync::Arc;

use async_trait::async_trait;
use helixflow_gateway::{
    CostEstimate, MockProvider, Provider, ProviderCatalog, ProviderHealth, ProviderRequest,
    ProviderResult, ProviderResultValue, ProviderTaskHandle,
};

use super::{ManualRunRequest, RunService};
use crate::tests::{executable_graph, open_temp_store, workspace_version};

/// Records every ProviderRequest while delegating behavior to mock (HF-003).
#[derive(Clone, Default)]
struct RecordingProvider {
    inner: MockProvider,
    requests: Arc<std::sync::Mutex<Vec<ProviderRequest>>>,
}

#[async_trait]
impl Provider for RecordingProvider {
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
        self.requests
            .lock()
            .expect("requests lock")
            .push(req.clone());
        self.inner.invoke(req).await
    }

    async fn cancel(&self, handle: ProviderTaskHandle) -> ProviderResultValue<()> {
        self.inner.cancel(handle).await
    }
}

#[tokio::test]
async fn providers_receive_materialized_upstream_text_inputs() {
    let (store, _dir) = open_temp_store().await;
    let (workspace_id, version_id) = workspace_version(&store).await;
    let provider = RecordingProvider::default();
    let requests = provider.requests.clone();
    let service = RunService::with_provider(store.clone(), provider);

    let outcome = service
        .execute_manual_run(ManualRunRequest {
            workspace_id,
            version_id,
            group_id: None,
            label: "Wired inputs".to_owned(),
            provider: "mock".to_owned(),
            graph: executable_graph(),
            force_rerun: false,
        })
        .await
        .expect("execute run");
    assert_eq!(outcome.run.status, "succeeded");

    let requests = requests.lock().expect("requests lock").clone();
    let writer = requests
        .iter()
        .find(|req| req.capability == "prompt_writer")
        .expect("prompt_writer request");
    assert_eq!(
        writer.input_texts.get("text").map(String::as_str),
        Some("launch teaser"),
        "prompt_writer must receive the wired input.text content"
    );
    let video = requests
        .iter()
        .find(|req| req.capability == "text_to_video")
        .expect("video request");
    let wired_prompt = video
        .input_texts
        .get("prompt")
        .expect("video prompt must be wired from prompt_writer output");
    assert!(
        wired_prompt.contains("provider=mock"),
        "wired prompt must come from the upstream artifact content, got: {wired_prompt}"
    );
}
