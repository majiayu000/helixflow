use helixflow_graph::{GraphService, WorkflowGraph};
use helixflow_registry::NodeRegistry;
use helixflow_store::VersionRecord;

use crate::api_error::ApiError;
use crate::catalog_routes::shared_catalog;

pub(crate) fn validate_legacy_migration_source(graph: &WorkflowGraph) -> Result<(), String> {
    let service = GraphService::new(NodeRegistry::builtin());
    let mut validation = graph.clone();
    for node in validation.nodes.values_mut() {
        let executable = service
            .registry()
            .definition(&node.node_type)
            .ok()
            .and_then(|definition| definition.capability.as_ref())
            .is_some();
        if executable
            && node
                .params
                .get("model")
                .is_some_and(serde_json::Value::is_string)
            && let Some(params) = node.params.as_object_mut()
        {
            params.remove("model");
        }
    }
    service
        .validate_graph(&validation)
        .map_err(|error| error.to_string())
}

pub(crate) fn validate_migration_candidate(graph: &WorkflowGraph) -> Result<(), String> {
    graph
        .validate_semantics(
            &GraphService::new(NodeRegistry::builtin()),
            shared_catalog(),
        )
        .map_err(|error| error.to_string())
}

pub(crate) fn derive_semantics_json(
    source: &VersionRecord,
    _before: &WorkflowGraph,
    after: &WorkflowGraph,
) -> Result<Option<String>, ApiError> {
    if source.semantics_json.is_none() {
        return Ok(None);
    }
    validate_migration_candidate(after).map_err(|error| {
        ApiError::conflict_with_details(
            "derived executable graph requires valid embedded semantics",
            serde_json::json!({"code": "SEMANTICS_INVALID", "reason": error}),
        )
    })?;

    serde_json::to_string(&after.collected_semantics())
        .map(Some)
        .map_err(|error| ApiError::server_error(format!("encode derived semantics: {error}")))
}
