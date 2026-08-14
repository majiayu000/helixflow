use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use helixflow_gateway::RuntimeProvider;
use helixflow_run::EventBus;
use helixflow_store::Store;

use super::*;
use crate::app_state::AppState;
use crate::test_support::FailingWorkbenchAgent;

async fn mock_state() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().expect("temp dir");
    let data_dir = dir.path().to_path_buf();
    let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    let state = AppState::with_store_agent_provider(
        EventBus::new(16),
        store,
        data_dir.clone(),
        Arc::new(FailingWorkbenchAgent),
        data_dir.join("sessions"),
        RuntimeProvider::mock(),
    );
    (dir, state)
}

#[tokio::test]
async fn catalog_endpoint_returns_revisioned_snapshot() {
    let Json(snapshot) = catalog_snapshot().await;

    assert!(snapshot.catalog_revision.starts_with("sha256:"));
    assert!(!snapshot.bindings.is_empty());
    let encoded = serde_json::to_value(snapshot).expect("serialize");
    assert!(encoded.get("catalogRevision").is_some());
    assert!(encoded.get("defaultBindings").is_some());
}

#[tokio::test]
async fn capability_models_projects_bindings() {
    let Json(payload) = capability_models(Path("text_to_image".to_owned()))
        .await
        .expect("models");

    let encoded = serde_json::to_value(&payload).expect("serialize");
    assert_eq!(encoded["capabilityId"], "text_to_image");
    assert!(
        encoded["models"]
            .as_array()
            .expect("models array")
            .iter()
            .any(|model| model["modelId"] == "google/nano-banana-2")
    );
}

#[tokio::test]
async fn unknown_capability_is_404_with_stable_code() {
    let err = capability_models(Path("style_transfer".to_owned()))
        .await
        .expect_err("unknown capability");

    assert_eq!(err.status, StatusCode::NOT_FOUND);
    assert!(err.message.contains("style_transfer"));
}

#[tokio::test]
async fn model_capabilities_accepts_slash_model_ids() {
    let Json(payload) = model_capabilities(Path("google/nano-banana-2".to_owned()))
        .await
        .expect("capabilities");

    let encoded = serde_json::to_value(&payload).expect("serialize");
    assert!(
        encoded["capabilities"]
            .as_array()
            .expect("capabilities array")
            .iter()
            .any(|capability| capability["capabilityId"] == "text_to_image")
    );
}

#[tokio::test]
async fn resolve_fails_closed_when_no_catalog_connector_is_healthy() {
    // The test registry only contains the mock provider, so the atlas/fal
    // connectors resolve as unavailable and the endpoint must fail closed.
    let (_dir, state) = mock_state().await;

    let err = resolve_implementation(
        State(state),
        Json(ResolveRequest {
            capability_id: "text_to_image".to_owned(),
            requested_model: Some("Nano Banana".to_owned()),
            connector_preference: None,
        }),
    )
    .await
    .expect_err("no healthy connector");

    assert_eq!(err.status, StatusCode::CONFLICT);
    assert!(err.message.contains("unavailable"));
}

#[tokio::test]
async fn workspace_resolution_uses_the_selected_provider_not_catalog_default() {
    let (_dir, mut state) = mock_state().await;
    let workspace = state
        .store
        .create_workspace("FAL workspace")
        .await
        .expect("workspace");
    state
        .store
        .set_workspace_runtime_provider(&workspace.id, Some("fal"))
        .await
        .expect("select fal");
    let atlas = RuntimeProvider::Atlas(helixflow_gateway::AtlasProvider::new(
        helixflow_gateway::ApiProviderConfig::atlas(
            "test-key".to_owned(),
            "https://atlas.invalid/v1".to_owned(),
        ),
    ));
    let fal = RuntimeProvider::Fal(helixflow_gateway::FalProvider::new(
        helixflow_gateway::FalProviderConfig::new(
            "test-key".to_owned(),
            "https://fal.invalid".to_owned(),
        ),
    ));
    state.provider_registry = helixflow_gateway::ProviderRegistry::new("atlas", vec![atlas, fal]);

    let Json(resolved) = resolve_workspace_implementation(
        Path(workspace.id),
        State(state),
        Json(ResolveRequest {
            capability_id: "text_to_image".to_owned(),
            requested_model: Some("Nano Banana".to_owned()),
            connector_preference: Some("atlas".to_owned()),
        }),
    )
    .await
    .expect("resolve selected provider");

    assert!(matches!(
        resolved.target,
        helixflow_registry::catalog::ImplementationTarget::ApiConnector {
            ref connector_id,
            ..
        } if connector_id == "fal"
    ));
}

#[tokio::test]
async fn connector_availability_maps_health_fail_closed() {
    let (_dir, state) = mock_state().await;
    let availability = connector_availability(&state.provider_registry.catalog_snapshot());

    // Mock is healthy in the test registry; atlas/fal are simply absent and
    // therefore unavailable to the resolver.
    assert_eq!(availability.get("mock").copied(), Some(true));
    assert_eq!(availability.get("atlas"), None);
    assert_eq!(availability.get("fal"), None);
}

#[tokio::test]
async fn compile_intent_clarifies_when_no_connector_is_healthy() {
    let (_dir, state) = mock_state().await;
    let request: CompileIntentRequest = serde_json::from_value(serde_json::json!({
        "intent": {
            "intentVersion": "1",
            "topology": "linear",
            "stages": [{
                "stageId": "s1",
                "capabilityId": "text_to_image",
                "requestedModel": "Nano Banana",
                "inputFrom": [],
                "params": { "prompt": "a product image" }
            }],
            "outputStageIds": ["s1"]
        }
    }))
    .expect("request parses");

    let err = compile_intent(State(state), Json(request))
        .await
        .expect_err("no healthy connectors");

    assert_eq!(err.status, StatusCode::CONFLICT);
    assert!(err.message.contains("BINDING_UNAVAILABLE"));
}
