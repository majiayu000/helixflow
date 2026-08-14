use helixflow_compiler::IntentPlan;
use helixflow_registry::resolver::ConnectorAvailability;

use crate::app_state::AppState;
use crate::capability_preflight::provider_capability_readiness;
use crate::catalog_routes::connector_availability;

pub(crate) struct IntentCompileReadiness {
    pub(crate) availability: ConnectorAvailability,
    pub(crate) connector_preference: Option<String>,
}

pub(crate) fn intent_compile_readiness(
    state: &AppState,
    selected_provider: &str,
    intent: &IntentPlan,
) -> Result<IntentCompileReadiness, String> {
    for stage in &intent.stages {
        let readiness =
            provider_capability_readiness(state, selected_provider, &stage.capability_id);
        if !readiness.runnable {
            return Err(readiness
                .code
                .unwrap_or_else(|| "PROVIDER_UNAVAILABLE".to_owned()));
        }
    }

    let catalog = helixflow_run::shared_catalog();
    let connector_preference = catalog
        .connector(selected_provider)
        .map(|_| selected_provider.to_owned());
    let availability = if connector_preference.is_some() {
        connector_availability(
            &state
                .provider_registry
                .catalog_snapshot_for_selected(Some(selected_provider)),
        )
    } else {
        // Direct dev/test providers execute canonical capabilities without a
        // production connector. Readiness above authorizes execution; enabled
        // catalog bindings are used only to construct a valid semantic graph.
        catalog
            .connectors
            .iter()
            .map(|connector| (connector.connector_id.clone(), connector.enabled))
            .collect()
    };
    Ok(IntentCompileReadiness {
        availability,
        connector_preference,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use helixflow_gateway::RuntimeProvider;
    use helixflow_run::EventBus;
    use helixflow_store::Store;

    use super::*;
    use crate::test_support::FailingWorkbenchAgent;

    async fn state(provider: RuntimeProvider) -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let store = Store::open(&format!(
            "sqlite://{}",
            data_dir.join("readiness.sqlite").display()
        ))
        .await
        .expect("store");
        let state = AppState::with_store_agent_provider(
            EventBus::new(8),
            store,
            data_dir.clone(),
            Arc::new(FailingWorkbenchAgent),
            data_dir.join("sessions"),
            provider,
        );
        (dir, state)
    }

    fn image_intent() -> IntentPlan {
        serde_json::from_value(serde_json::json!({
            "intentVersion": "1",
            "topology": "linear",
            "stages": [{
                "stageId": "image",
                "capabilityId": "text_to_image",
                "inputFrom": [],
                "params": { "prompt": "forest" }
            }],
            "outputStageIds": ["image"]
        }))
        .expect("intent")
    }

    #[tokio::test]
    async fn explicit_mock_uses_direct_readiness_then_canonical_graph_bindings() {
        let (_dir, state) = state(RuntimeProvider::mock()).await;
        let readiness =
            intent_compile_readiness(&state, "mock", &image_intent()).expect("mock readiness");

        assert_eq!(readiness.connector_preference, None);
        assert_eq!(readiness.availability.get("atlas"), Some(&true));
        assert_eq!(readiness.availability.get("fal"), Some(&true));
    }

    #[tokio::test]
    async fn unavailable_provider_fails_before_intent_compilation() {
        let (_dir, state) = state(RuntimeProvider::unavailable("atlas", "not configured")).await;
        let result = intent_compile_readiness(&state, "atlas", &image_intent());
        assert!(matches!(result, Err(reason) if reason == "PROVIDER_UNAVAILABLE"));
    }
}
