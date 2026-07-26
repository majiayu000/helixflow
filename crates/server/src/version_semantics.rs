use std::collections::BTreeMap;

use helixflow_graph::graph_v2::{GRAPH_SCHEMA_V2, NodeSemanticsEntry, WorkflowGraphV2};
use helixflow_graph::{GraphService, WorkflowGraph};
use helixflow_registry::NodeRegistry;
use helixflow_store::VersionRecord;
use serde_json::json;

use crate::api_error::ApiError;
use crate::catalog_routes::shared_catalog;

pub(crate) fn derive_semantics_json(
    source: &VersionRecord,
    before: &WorkflowGraph,
    after: &WorkflowGraph,
) -> Result<Option<String>, ApiError> {
    let Some(encoded) = source.semantics_json.as_deref() else {
        return Ok(None);
    };
    let mut semantics: BTreeMap<String, NodeSemanticsEntry> = serde_json::from_str(encoded)
        .map_err(|_| ApiError::conflict("source version semantics are invalid"))?;
    let registry = NodeRegistry::builtin();

    semantics.retain(|node_id, _| after.nodes.contains_key(node_id));
    for (node_id, node) in &after.nodes {
        let executable = registry
            .definition(&node.node_type)
            .map_err(|_| ApiError::conflict("derived graph contains an unknown node type"))?
            .capability
            .is_some();
        if executable {
            let unchanged_type = before
                .nodes
                .get(node_id)
                .is_some_and(|old| old.node_type == node.node_type);
            if !unchanged_type || !semantics.contains_key(node_id) {
                return Err(ApiError::conflict_with_details(
                    "derived executable node requires explicit graph v2 semantics",
                    json!({"code": "EXPLICIT_SEMANTICS_REQUIRED", "nodeId": node_id}),
                ));
            }
        } else {
            semantics.remove(node_id);
        }
    }

    WorkflowGraphV2 {
        schema_version: GRAPH_SCHEMA_V2,
        catalog_revision: shared_catalog().catalog_revision.clone(),
        base: after.clone(),
        semantics: semantics.clone(),
    }
    .validate(&GraphService::new(registry), shared_catalog())
    .map_err(|error| {
        ApiError::conflict_with_details(
            "derived graph v2 semantics are invalid",
            json!({"code": error.code()}),
        )
    })?;

    serde_json::to_string(&semantics)
        .map(Some)
        .map_err(|error| ApiError::server_error(format!("encode derived semantics: {error}")))
}
