//! Run-time implementation resolution (GH130 T4, tech.md §8).
//!
//! At run creation every model-bearing step is resolved against the builtin
//! catalog and the workspace-selected connector, and the result is frozen
//! into the plan (persisted via `runs.plan_json`, reused verbatim by
//! retries/sweeps — P11/P12). Providers refuse to execute without it, so a
//! silent provider default can never reappear.

use std::sync::OnceLock;

use helixflow_graph::graph_v2::{NodeSemanticsEntry, canonical_capability};
use helixflow_graph::{ExecutionPlan, ResolvedStepBinding};
use helixflow_registry::catalog::{CatalogSnapshot, ImplementationSelection, ImplementationTarget};
use helixflow_registry::catalog_seed::builtin_catalog;
use helixflow_registry::resolver::{CapabilityResolver, ConnectorAvailability, ResolveRequest};
use std::collections::BTreeMap;

use crate::error::RunError;

pub fn shared_catalog() -> &'static CatalogSnapshot {
    static CATALOG: OnceLock<CatalogSnapshot> = OnceLock::new();
    CATALOG.get_or_init(builtin_catalog)
}

/// Resolves one capability against a specific connector. Pinned selections
/// (from a semantic layer entry) must resolve to exactly their binding and
/// model (P4); policy selections use the configured defaults, restricted to
/// the connector actually executing the run.
pub fn resolve_step_binding(
    catalog: &CatalogSnapshot,
    legacy_capability: &str,
    connector_id: &str,
    semantics: Option<&NodeSemanticsEntry>,
) -> Result<ResolvedStepBinding, (String, String)> {
    let capability_id = canonical_capability(legacy_capability).to_owned();
    let availability: ConnectorAvailability = BTreeMap::from([(connector_id.to_owned(), true)]);
    let resolver = CapabilityResolver::new(catalog);

    let requested_model = match semantics.map(|entry| &entry.implementation) {
        Some(ImplementationSelection::Pinned {
            requested_model_id, ..
        }) => Some(requested_model_id.clone()),
        _ => None,
    };
    let resolved = resolver
        .resolve(
            &ResolveRequest {
                capability_id: capability_id.clone(),
                requested_model: requested_model.clone(),
                connector_preference: Some(connector_id.to_owned()),
            },
            &availability,
        )
        .map_err(|err| (err.code().to_owned(), err.to_string()))?;

    if let Some(ImplementationSelection::Pinned {
        requested_model_id,
        binding_id,
    }) = semantics.map(|entry| &entry.implementation)
    {
        if &resolved.resolved_model_id != requested_model_id || &resolved.binding_id != binding_id {
            return Err((
                "PINNED_MODEL_MISMATCH".to_owned(),
                format!(
                    "pinned `{requested_model_id}` via `{binding_id}` but resolution produced `{}` via `{}`",
                    resolved.resolved_model_id, resolved.binding_id
                ),
            ));
        }
    }

    let ImplementationTarget::ApiConnector {
        connector_id: resolved_connector,
        operation_id,
    } = &resolved.target
    else {
        return Err((
            "BINDING_UNAVAILABLE".to_owned(),
            format!(
                "binding `{}` has no API connector target",
                resolved.binding_id
            ),
        ));
    };

    Ok(ResolvedStepBinding {
        capability_id,
        requested_model_id: resolved.requested_model_id,
        resolved_model_id: resolved.resolved_model_id,
        binding_id: resolved.binding_id,
        binding_revision: resolved.binding_revision,
        connector_id: resolved_connector.clone(),
        operation_id: operation_id.clone(),
    })
}

/// Capability-level resolution entry for the server preflight: same catalog,
/// same rules as the run-time freeze, without needing a compiled plan.
pub fn resolve_step_binding_for(
    legacy_capability: &str,
    connector_id: &str,
    semantics: Option<&NodeSemanticsEntry>,
) -> Result<ResolvedStepBinding, (String, String)> {
    resolve_step_binding(shared_catalog(), legacy_capability, connector_id, semantics)
}

/// Loads the persisted semantic layer for a version, if any. Legacy v1
/// versions return `None` and resolve through configured policy defaults.
pub async fn load_version_semantics(
    store: &helixflow_store::Store,
    version_id: &str,
) -> Result<Option<BTreeMap<String, NodeSemanticsEntry>>, RunError> {
    let version = store.version(version_id).await?;
    let Some(raw) = version.semantics_json.as_deref() else {
        return Ok(None);
    };
    let semantics = serde_json::from_str(raw)?;
    Ok(Some(semantics))
}

/// Freezes resolved implementations into a freshly compiled plan. Providers
/// outside the catalog (e.g. mock) are skipped; catalog connectors fail
/// closed on any unresolvable step. `semantics` carries the graph's semantic
/// layer once durable v2 graphs exist (T6); `None` resolves via policy
/// defaults only.
pub fn attach_resolved_bindings(
    plan: &mut ExecutionPlan,
    connector_id: &str,
    semantics: Option<&BTreeMap<String, NodeSemanticsEntry>>,
) -> Result<(), RunError> {
    let catalog = shared_catalog();
    if catalog.connector(connector_id).is_none() {
        return Ok(());
    }

    for step in &mut plan.steps {
        let Some(capability) = step.capability.as_deref() else {
            continue;
        };
        let entry = semantics.and_then(|map| map.get(&step.node_id));
        let resolved = resolve_step_binding(catalog, capability, connector_id, entry).map_err(
            |(code, message)| RunError::ResolutionFailed {
                node_id: step.node_id.clone(),
                code,
                message,
            },
        )?;
        step.resolved = Some(resolved);
    }
    plan.catalog_revision = Some(catalog.catalog_revision.clone());
    Ok(())
}

/// Builds the `run.resolved_implementations` audit event payload (GH130 T4):
/// requested/resolved model, binding, and connector per step, plus the frozen
/// catalog revision. `None` when the plan carries no resolved bindings.
pub(crate) fn resolved_implementations_event(plan: &ExecutionPlan) -> Option<serde_json::Value> {
    let steps: Vec<_> = plan
        .steps
        .iter()
        .filter_map(|step| {
            step.resolved.as_ref().map(|resolved| {
                serde_json::json!({
                    "nodeId": step.node_id,
                    "capabilityId": resolved.capability_id,
                    "requestedModelId": resolved.requested_model_id,
                    "resolvedModelId": resolved.resolved_model_id,
                    "bindingId": resolved.binding_id,
                    "bindingRevision": resolved.binding_revision,
                    "connectorId": resolved.connector_id,
                    "operationId": resolved.operation_id,
                })
            })
        })
        .collect();
    if steps.is_empty() {
        return None;
    }
    Some(serde_json::json!({
        "catalogRevision": plan.catalog_revision,
        "steps": steps,
    }))
}

#[cfg(test)]
#[path = "resolved_tests.rs"]
mod tests;
