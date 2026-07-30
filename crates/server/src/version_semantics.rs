use std::collections::BTreeMap;

use helixflow_graph::semantics::NodeSemanticsEntry;
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
        .validate_graph_for_migration(&validation)
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

pub(crate) fn canonical_semantics_graph(
    graph: &WorkflowGraph,
    sidecar_json: Option<&str>,
) -> Result<Option<WorkflowGraph>, String> {
    if graph.catalog_revision.is_some() {
        return Ok(Some(graph.clone()));
    }
    if let Some(encoded) = sidecar_json {
        let semantics: BTreeMap<String, NodeSemanticsEntry> =
            serde_json::from_str(encoded).map_err(|error| error.to_string())?;
        return layer_sidecar_semantics(graph, semantics).map(Some);
    }
    Ok(graph
        .nodes
        .values()
        .any(|node| node.semantics.is_some())
        .then(|| graph.clone()))
}

fn layer_sidecar_semantics(
    graph: &WorkflowGraph,
    semantics: BTreeMap<String, NodeSemanticsEntry>,
) -> Result<WorkflowGraph, String> {
    let mut layered = graph.clone();
    layered.catalog_revision = Some(shared_catalog().catalog_revision.clone());
    for (node_id, entry) in semantics {
        let node = layered
            .nodes
            .get_mut(&node_id)
            .ok_or_else(|| format!("semantics references unknown node `{node_id}`"))?;
        node.semantics = Some(entry);
    }
    Ok(layered)
}

pub(crate) fn derive_semantics_json(
    source: &VersionRecord,
    _before: &WorkflowGraph,
    after: &WorkflowGraph,
) -> Result<Option<String>, ApiError> {
    let canonical = if after.catalog_revision.is_some() {
        after.clone()
    } else if let Some(sidecar_json) = source.semantics_json.as_deref() {
        let mut semantics: BTreeMap<String, NodeSemanticsEntry> =
            serde_json::from_str(sidecar_json).map_err(|error| {
                derived_semantics_error(format!("decode source semantics: {error}"))
            })?;
        semantics.retain(|node_id, _| after.nodes.contains_key(node_id));
        layer_sidecar_semantics(after, semantics).map_err(derived_semantics_error)?
    } else if after.nodes.values().any(|node| node.semantics.is_some()) {
        after.clone()
    } else {
        return Ok(None);
    };
    validate_migration_candidate(&canonical).map_err(derived_semantics_error)?;

    serde_json::to_string(&canonical.collected_semantics())
        .map(Some)
        .map_err(|error| ApiError::server_error(format!("encode derived semantics: {error}")))
}

fn derived_semantics_error(error: String) -> ApiError {
    ApiError::conflict_with_details(
        "derived executable graph requires valid embedded semantics",
        serde_json::json!({"code": "SEMANTICS_INVALID", "reason": error}),
    )
}
