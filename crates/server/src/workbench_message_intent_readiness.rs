use helixflow_agent::{CanvasEditOp, CanvasEditPlan};
use helixflow_registry::NodeRegistry;
use helixflow_registry::resolver::ConnectorAvailability;

use crate::app_state::AppState;
use crate::capability_preflight::provider_capability_readiness;
use crate::catalog_routes::connector_availability;

pub(crate) struct CanvasCompileReadiness {
    pub(crate) availability: ConnectorAvailability,
    pub(crate) connector_preference: Option<String>,
}

pub(crate) fn canvas_edit_readiness(
    state: &AppState,
    selected_provider: &str,
    edit: &CanvasEditPlan,
) -> Result<CanvasCompileReadiness, String> {
    let registry = NodeRegistry::builtin();
    for operation in &edit.operations {
        let CanvasEditOp::AddNode { node_type, .. } = operation else {
            continue;
        };
        let Ok(definition) = registry.definition(node_type) else {
            continue;
        };
        let Some(capability_id) = definition.capability.as_deref() else {
            continue;
        };
        let readiness = provider_capability_readiness(state, selected_provider, capability_id);
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
    Ok(CanvasCompileReadiness {
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

    fn image_edit() -> CanvasEditPlan {
        serde_json::from_value(serde_json::json!({
            "operations": [{
                "op": "add_node",
                "id": "image",
                "node_type": "image.generate",
                "params": { "prompt": "forest" }
            }]
        }))
        .expect("canvas edit")
    }

    #[tokio::test]
    async fn explicit_mock_uses_direct_readiness_then_canonical_graph_bindings() {
        let (_dir, state) = state(RuntimeProvider::mock()).await;
        let readiness =
            canvas_edit_readiness(&state, "mock", &image_edit()).expect("mock readiness");

        assert_eq!(readiness.connector_preference, None);
        assert_eq!(readiness.availability.get("atlas"), Some(&true));
        assert_eq!(readiness.availability.get("fal"), Some(&true));
    }

    #[tokio::test]
    async fn unavailable_provider_fails_before_canvas_compilation() {
        let (_dir, state) = state(RuntimeProvider::unavailable("atlas", "not configured")).await;
        let result = canvas_edit_readiness(&state, "atlas", &image_edit());
        assert!(matches!(result, Err(reason) if reason == "PROVIDER_UNAVAILABLE"));
    }
}
