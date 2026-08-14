use helixflow_graph::WorkflowGraph;
use helixflow_registry::NodeRegistry;
use serde::Serialize;

use crate::api_error::ApiError;
use crate::app_state::AppState;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderCapabilityReadiness {
    pub(crate) capability_id: String,
    pub(crate) provider_id: String,
    pub(crate) runnable: bool,
    pub(crate) mode: &'static str,
    pub(crate) code: Option<String>,
    pub(crate) message: Option<String>,
}

pub(crate) fn provider_capability_readiness(
    state: &AppState,
    provider: &str,
    capability: &str,
) -> ProviderCapabilityReadiness {
    let snapshot = state
        .provider_registry
        .catalog_snapshot_for_selected(Some(provider));
    let summary = snapshot
        .runtime_providers
        .iter()
        .find(|summary| summary.id == provider);
    let unavailable = match summary {
        None => Some((
            "PROVIDER_UNAVAILABLE".to_owned(),
            format!("provider `{provider}` is not registered"),
        )),
        Some(summary) if !summary.enabled || summary.status != "healthy" => Some((
            "PROVIDER_UNAVAILABLE".to_owned(),
            summary
                .message
                .clone()
                .unwrap_or_else(|| format!("provider `{provider}` is unavailable")),
        )),
        Some(summary) if !summary.capabilities.iter().any(|item| item == capability) => Some((
            "CAPABILITY_UNSUPPORTED".to_owned(),
            format!("provider `{provider}` does not support capability `{capability}`"),
        )),
        Some(_) => None,
    };
    let mode = if helixflow_run::shared_catalog()
        .connector(provider)
        .is_some()
    {
        "catalog"
    } else {
        "direct"
    };
    if let Some((code, message)) = unavailable {
        return ProviderCapabilityReadiness {
            capability_id: capability.to_owned(),
            provider_id: provider.to_owned(),
            runnable: false,
            mode,
            code: Some(code),
            message: Some(message),
        };
    }
    if mode == "catalog"
        && let Err((code, message)) =
            helixflow_run::resolve_step_binding_for(capability, provider, None)
    {
        return ProviderCapabilityReadiness {
            capability_id: capability.to_owned(),
            provider_id: provider.to_owned(),
            runnable: false,
            mode,
            code: Some(code),
            message: Some(message),
        };
    }
    ProviderCapabilityReadiness {
        capability_id: capability.to_owned(),
        provider_id: provider.to_owned(),
        runnable: true,
        mode,
        code: None,
        message: None,
    }
}

/// Reject a run before it is queued when the selected provider does not
/// support every capability the graph needs (HF-009). Without this check,
/// graphs enqueue successfully and only fail mid-execution.
pub(crate) fn preflight_provider_capabilities(
    state: &AppState,
    provider: &str,
    graph: &WorkflowGraph,
) -> Result<(), ApiError> {
    let registry = NodeRegistry::builtin();
    for (node_id, node) in &graph.nodes {
        let Ok(definition) = registry.definition(&node.node_type) else {
            continue;
        };
        let Some(capability) = definition.capability.as_deref() else {
            continue;
        };
        let readiness = provider_capability_readiness(state, provider, capability);
        if !readiness.runnable {
            let code = readiness.code.as_deref().unwrap_or("PROVIDER_UNAVAILABLE");
            let message = readiness
                .message
                .as_deref()
                .unwrap_or("provider capability is unavailable");
            return Err(ApiError::conflict_with_details(
                format!(
                    "node `{node_id}` ({}) is not runnable: {message}",
                    node.node_type
                ),
                serde_json::json!({
                    "code": code,
                    "nodeId": node_id,
                    "providerId": provider,
                    "capabilityId": capability,
                }),
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
        assert_eq!(
            provider_capability_readiness(&state, "mock", "text_to_video"),
            ProviderCapabilityReadiness {
                capability_id: "text_to_video".to_owned(),
                provider_id: "mock".to_owned(),
                runnable: true,
                mode: "direct",
                code: None,
                message: None,
            }
        );
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
