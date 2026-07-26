use helixflow_graph::WorkflowGraph;
use helixflow_registry::NodeRegistry;

use crate::api_error::ApiError;
use crate::app_state::AppState;

/// Reject a run before it is queued when the selected provider does not
/// support every capability the graph needs (HF-009). Without this check,
/// graphs enqueue successfully and only fail mid-execution.
pub(crate) fn preflight_provider_capabilities(
    state: &AppState,
    provider: &str,
    graph: &WorkflowGraph,
) -> Result<(), ApiError> {
    let registry = NodeRegistry::builtin();
    let snapshot = state.provider_registry.catalog_snapshot();
    let provider_capabilities = snapshot
        .runtime_providers
        .iter()
        .find(|summary| summary.id == provider)
        .map(|summary| summary.capabilities.clone())
        .unwrap_or_default();
    for (node_id, node) in &graph.nodes {
        let Ok(definition) = registry.definition(&node.node_type) else {
            continue;
        };
        let Some(capability) = definition.capability.as_deref() else {
            continue;
        };
        if !provider_capabilities.iter().any(|item| item == capability) {
            return Err(ApiError::conflict(format!(
                "provider `{provider}` does not support capability `{capability}` \
                 required by node `{node_id}` ({})",
                node.node_type
            )));
        }
        // GH130 T4: for catalog connectors the implementation must resolve
        // before the run is queued — the same resolution the run will freeze
        // into its plan, so nothing can drift between preflight and execute.
        if helixflow_run::shared_catalog()
            .connector(provider)
            .is_some()
            && let Err((code, message)) =
                helixflow_run::resolve_step_binding_for(capability, provider, None)
        {
            return Err(ApiError::conflict_with_details(
                format!("implementation for node `{node_id}` cannot be resolved: {message}"),
                serde_json::json!({ "code": code, "nodeId": node_id }),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use helixflow_gateway::RuntimeProvider;
    use helixflow_graph::{GraphNode, WorkflowGraph};
    use helixflow_run::EventBus;
    use helixflow_store::Store;
    use serde_json::json;

    use super::*;
    use crate::app_state::AppState;
    use crate::test_support::FailingWorkbenchAgent;

    fn video_graph() -> WorkflowGraph {
        WorkflowGraph {
            schema_version: 1,
            nodes: BTreeMap::from([(
                "video".to_owned(),
                GraphNode {
                    node_type: "video.text_to_video".to_owned(),
                    title: "Video".to_owned(),
                    params: json!({
                        "prompt": "clip",
                        "duration_sec": 4,
                        "aspect_ratio": "9:16"
                    }),
                    pos: [0.0, 0.0],
                    size: None,
                    semantics: None,
                },
            )]),
            edges: Vec::new(),
            catalog_revision: None,
        }
    }

    async fn state_with_provider(provider: RuntimeProvider) -> (tempfile::TempDir, AppState) {
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
            provider,
        );
        (dir, state)
    }

    #[tokio::test]
    async fn mock_provider_supports_video_graph() {
        let (_dir, state) = state_with_provider(RuntimeProvider::mock()).await;
        assert!(preflight_provider_capabilities(&state, "mock", &video_graph()).is_ok());
    }

    #[tokio::test]
    async fn unsupported_capability_is_rejected_before_queueing() {
        let (_dir, state) = state_with_provider(RuntimeProvider::mock()).await;
        let err = preflight_provider_capabilities(&state, "missing_provider", &video_graph())
            .expect_err("unknown provider must fail preflight");
        assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
        assert!(err.message.contains("text_to_video"));
    }
}
