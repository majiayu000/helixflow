//! Node-embedded semantic layer over the structural graph (GH145).
//!
//! The former layered `WorkflowGraphV2` (base + side-car semantics map) is
//! merged into `WorkflowGraph`: each executable node carries its own
//! catalog-pinned [`NodeSemanticsEntry`], and the graph carries the catalog
//! revision those entries were resolved against. Legacy graphs deserialize
//! with both fields absent and migrate through [`migrate_v1`].

use std::collections::BTreeMap;

use helixflow_registry::NodeRegistry;
use helixflow_registry::catalog::{CatalogSnapshot, ImplementationSelection};
use helixflow_registry::resolver::{
    CapabilityResolver, ConnectorAvailability, ResolveError, ResolveRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{GraphService, WorkflowGraph};

pub const MIGRATION_VERSION: &str = "1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeSemanticsEntry {
    pub capability_id: String,
    pub mode: String,
    pub implementation: ImplementationSelection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticsError {
    CatalogRevisionMissing,
    CatalogRevisionStale {
        graph_revision: String,
        catalog_revision: String,
    },
    StructuralInvalid(String),
    MissingSemantics(String),
    UnexpectedSemantics(String),
    CapabilityNotFound {
        node_id: String,
        capability_id: String,
    },
    CapabilityMismatch {
        node_id: String,
        node_type: String,
        capability_id: String,
    },
    BindingNotFound {
        node_id: String,
        binding_id: String,
    },
    BindingMismatch {
        node_id: String,
        binding_id: String,
        reason: String,
    },
    PinnedModelMismatch {
        node_id: String,
        requested_model_id: String,
        binding_model_id: String,
    },
    PolicyDefaultMissing {
        node_id: String,
        capability_id: String,
    },
    ParamsInvalid {
        node_id: String,
        message: String,
    },
}

impl SemanticsError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::CatalogRevisionMissing => "CATALOG_REVISION_MISSING",
            Self::CatalogRevisionStale { .. } => "CATALOG_REVISION_STALE",
            Self::StructuralInvalid(_) => "STRUCTURAL_INVALID",
            Self::MissingSemantics(_) => "MISSING_SEMANTICS",
            Self::UnexpectedSemantics(_) => "UNEXPECTED_SEMANTICS",
            Self::CapabilityNotFound { .. } => "CAPABILITY_NOT_FOUND",
            Self::CapabilityMismatch { .. } => "MODEL_CAPABILITY_MISMATCH",
            Self::BindingNotFound { .. } | Self::BindingMismatch { .. } => "BINDING_NOT_FOUND",
            Self::PinnedModelMismatch { .. } => "PINNED_MODEL_MISMATCH",
            Self::PolicyDefaultMissing { .. } => "BINDING_AMBIGUOUS",
            Self::ParamsInvalid { .. } => "REQUIRED_INPUT_MISSING",
        }
    }
}

impl std::fmt::Display for SemanticsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CatalogRevisionMissing => {
                write!(f, "graph carries node semantics but no catalog revision")
            }
            Self::CatalogRevisionStale {
                graph_revision,
                catalog_revision,
            } => write!(
                f,
                "graph catalog revision `{graph_revision}` does not match catalog `{catalog_revision}`"
            ),
            Self::StructuralInvalid(message) => {
                write!(f, "structural validation failed: {message}")
            }
            Self::MissingSemantics(node_id) => {
                write!(f, "executable node `{node_id}` has no semantics entry")
            }
            Self::UnexpectedSemantics(node_id) => {
                write!(f, "semantics entry on non-executable node `{node_id}`")
            }
            Self::CapabilityNotFound {
                node_id,
                capability_id,
            } => write!(
                f,
                "node `{node_id}` references capability `{capability_id}` not in catalog"
            ),
            Self::CapabilityMismatch {
                node_id,
                node_type,
                capability_id,
            } => write!(
                f,
                "node `{node_id}` of type `{node_type}` cannot carry capability `{capability_id}`"
            ),
            Self::BindingNotFound {
                node_id,
                binding_id,
            } => write!(
                f,
                "node `{node_id}` references unknown binding `{binding_id}`"
            ),
            Self::BindingMismatch {
                node_id,
                binding_id,
                reason,
            } => write!(
                f,
                "node `{node_id}` binding `{binding_id}` is inconsistent: {reason}"
            ),
            Self::PinnedModelMismatch {
                node_id,
                requested_model_id,
                binding_model_id,
            } => write!(
                f,
                "node `{node_id}` pinned `{requested_model_id}` but binding executes `{binding_model_id}`"
            ),
            Self::PolicyDefaultMissing {
                node_id,
                capability_id,
            } => write!(
                f,
                "node `{node_id}` uses policy selection but capability `{capability_id}` has no configured default binding"
            ),
            Self::ParamsInvalid { node_id, message } => {
                write!(
                    f,
                    "node `{node_id}` params do not satisfy the binding schema: {message}"
                )
            }
        }
    }
}

impl std::error::Error for SemanticsError {}

impl WorkflowGraph {
    /// Collects the embedded semantic layer keyed by node id. Empty for
    /// legacy graphs that never migrated.
    pub fn collected_semantics(&self) -> BTreeMap<String, NodeSemanticsEntry> {
        self.nodes
            .iter()
            .filter_map(|(node_id, node)| {
                node.semantics
                    .as_ref()
                    .map(|entry| (node_id.clone(), entry.clone()))
            })
            .collect()
    }

    /// Validates the structural layer and every embedded semantics entry
    /// against a specific catalog revision. Fail-closed on every
    /// inconsistency (P8).
    pub fn validate_semantics(
        &self,
        service: &GraphService,
        catalog: &CatalogSnapshot,
    ) -> Result<(), SemanticsError> {
        let Some(graph_revision) = self.catalog_revision.as_deref() else {
            return Err(SemanticsError::CatalogRevisionMissing);
        };
        if graph_revision != catalog.catalog_revision {
            return Err(SemanticsError::CatalogRevisionStale {
                graph_revision: graph_revision.to_owned(),
                catalog_revision: catalog.catalog_revision.clone(),
            });
        }
        service
            .validate_graph(self)
            .map_err(|err| SemanticsError::StructuralInvalid(err.to_string()))?;

        let registry = service.registry();
        for (node_id, node) in &self.nodes {
            let definition = registry
                .definition(&node.node_type)
                .map_err(|err| SemanticsError::StructuralInvalid(err.to_string()))?;
            match &definition.capability {
                Some(capability) => {
                    let Some(entry) = node.semantics.as_ref() else {
                        return Err(SemanticsError::MissingSemantics(node_id.clone()));
                    };
                    let wired: std::collections::BTreeSet<String> = self
                        .edges
                        .iter()
                        .filter(|edge| edge.to[0] == *node_id)
                        .map(|edge| edge.to[1].clone())
                        .collect();
                    validate_entry(
                        node_id,
                        &node.node_type,
                        &node.params,
                        &wired,
                        capability,
                        entry,
                        catalog,
                    )?;
                }
                None => {
                    if node.semantics.is_some() {
                        return Err(SemanticsError::UnexpectedSemantics(node_id.clone()));
                    }
                }
            }
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_entry(
    node_id: &str,
    node_type: &str,
    params: &Value,
    wired: &std::collections::BTreeSet<String>,
    expected_capability: &str,
    entry: &NodeSemanticsEntry,
    catalog: &CatalogSnapshot,
) -> Result<(), SemanticsError> {
    if entry.capability_id != expected_capability {
        return Err(SemanticsError::CapabilityMismatch {
            node_id: node_id.to_owned(),
            node_type: node_type.to_owned(),
            capability_id: entry.capability_id.clone(),
        });
    }
    if catalog.capability(&entry.capability_id).is_none() {
        return Err(SemanticsError::CapabilityNotFound {
            node_id: node_id.to_owned(),
            capability_id: entry.capability_id.clone(),
        });
    }

    match &entry.implementation {
        ImplementationSelection::Pinned {
            requested_model_id,
            binding_id,
        } => {
            let Some(binding) = catalog.binding(binding_id) else {
                return Err(SemanticsError::BindingNotFound {
                    node_id: node_id.to_owned(),
                    binding_id: binding_id.clone(),
                });
            };
            if binding.capability_id != entry.capability_id {
                return Err(SemanticsError::BindingMismatch {
                    node_id: node_id.to_owned(),
                    binding_id: binding_id.clone(),
                    reason: format!(
                        "binding implements `{}`, node declares `{}`",
                        binding.capability_id, entry.capability_id
                    ),
                });
            }
            if &binding.model_id != requested_model_id {
                return Err(SemanticsError::PinnedModelMismatch {
                    node_id: node_id.to_owned(),
                    requested_model_id: requested_model_id.clone(),
                    binding_model_id: binding.model_id.clone(),
                });
            }
            if binding.mode != entry.mode {
                return Err(SemanticsError::BindingMismatch {
                    node_id: node_id.to_owned(),
                    binding_id: binding_id.clone(),
                    reason: format!(
                        "binding mode `{}` does not match node mode `{}`",
                        binding.mode, entry.mode
                    ),
                });
            }
            binding
                .input_schema
                .validate_value_with_wired(binding_id, params, wired)
                .map_err(|err| SemanticsError::ParamsInvalid {
                    node_id: node_id.to_owned(),
                    message: err.to_string(),
                })?;
        }
        ImplementationSelection::Policy { .. } => {
            let Some(default_id) = catalog.default_bindings.get(&entry.capability_id) else {
                return Err(SemanticsError::PolicyDefaultMissing {
                    node_id: node_id.to_owned(),
                    capability_id: entry.capability_id.clone(),
                });
            };
            if catalog.binding(default_id).is_none() {
                return Err(SemanticsError::BindingNotFound {
                    node_id: node_id.to_owned(),
                    binding_id: default_id.clone(),
                });
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationReport {
    pub migration_version: String,
    pub source_schema_version: u32,
    pub catalog_revision: String,
    pub nodes: Vec<NodeMigration>,
    pub resolvable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeMigration {
    pub node_id: String,
    pub action: MigrationAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum MigrationAction {
    Structural,
    #[serde(rename_all = "camelCase")]
    MappedPolicy {
        capability_id: String,
    },
    #[serde(rename_all = "camelCase")]
    MappedPinned {
        capability_id: String,
        model_id: String,
        binding_id: String,
    },
    #[serde(rename_all = "camelCase")]
    NeedsResolution {
        reason: String,
    },
    #[serde(rename_all = "camelCase")]
    NeedsUserChoice {
        candidates: Vec<String>,
    },
}

/// Migrates a legacy graph (no embedded semantics) into the canonical
/// embedded shape. Deterministic and dry-run friendly: the report is always
/// produced; the graph is only produced when every executable node is
/// resolvable. Never infers a model from titles (P14) and never adopts a
/// provider-internal default: nodes without a declared model become explicit
/// `Policy` selections, which resolve only through the configured default
/// binding (P5).
pub fn migrate_v1(
    graph: &WorkflowGraph,
    registry: &NodeRegistry,
    catalog: &CatalogSnapshot,
) -> (Option<WorkflowGraph>, MigrationReport) {
    // Migration decides against catalog data, not runtime health, so every
    // declared connector counts as reachable for binding selection.
    let all_available: ConnectorAvailability = catalog
        .connectors
        .iter()
        .map(|connector| (connector.connector_id.clone(), true))
        .collect();
    let resolver = CapabilityResolver::new(catalog);

    let mut migrated = graph.clone();
    let mut nodes = Vec::new();
    let mut resolvable = true;

    for (node_id, node) in &graph.nodes {
        let action = match registry.definition(&node.node_type) {
            Err(err) => {
                resolvable = false;
                MigrationAction::NeedsResolution {
                    reason: format!("unknown node type: {err}"),
                }
            }
            Ok(definition) => match &definition.capability {
                None => MigrationAction::Structural,
                Some(capability) => {
                    let capability_id = capability.to_owned();
                    let legacy_model = node
                        .params
                        .get("model")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    match migrate_executable_node(
                        &capability_id,
                        legacy_model.as_deref(),
                        &resolver,
                        &all_available,
                        catalog,
                    ) {
                        Ok((entry, action)) => {
                            if let Some(node) = migrated.nodes.get_mut(node_id) {
                                if legacy_model.is_some()
                                    && let Some(params) = node.params.as_object_mut()
                                {
                                    params.remove("model");
                                }
                                node.semantics = Some(entry);
                            }
                            action
                        }
                        Err(action) => {
                            resolvable = false;
                            action
                        }
                    }
                }
            },
        };
        nodes.push(NodeMigration {
            node_id: node_id.clone(),
            action,
        });
    }

    let report = MigrationReport {
        migration_version: MIGRATION_VERSION.to_owned(),
        source_schema_version: graph.schema_version,
        catalog_revision: catalog.catalog_revision.clone(),
        nodes,
        resolvable,
    };
    let migrated = resolvable.then(|| {
        migrated.catalog_revision = Some(catalog.catalog_revision.clone());
        migrated
    });
    (migrated, report)
}

fn migrate_executable_node(
    capability_id: &str,
    legacy_model: Option<&str>,
    resolver: &CapabilityResolver<'_>,
    all_available: &ConnectorAvailability,
    catalog: &CatalogSnapshot,
) -> Result<(NodeSemanticsEntry, MigrationAction), MigrationAction> {
    if catalog.capability(capability_id).is_none() {
        return Err(MigrationAction::NeedsResolution {
            reason: format!("capability `{capability_id}` is not in the catalog"),
        });
    }

    match legacy_model {
        Some(model) => {
            let request = ResolveRequest {
                capability_id: capability_id.to_owned(),
                requested_model: Some(model.to_owned()),
                connector_preference: None,
            };
            match resolver.resolve(&request, all_available) {
                Ok(resolved) => Ok((
                    NodeSemanticsEntry {
                        capability_id: capability_id.to_owned(),
                        mode: capability_id.to_owned(),
                        implementation: ImplementationSelection::Pinned {
                            requested_model_id: resolved.resolved_model_id.clone(),
                            binding_id: resolved.binding_id.clone(),
                        },
                    },
                    MigrationAction::MappedPinned {
                        capability_id: capability_id.to_owned(),
                        model_id: resolved.resolved_model_id,
                        binding_id: resolved.binding_id,
                    },
                )),
                Err(ResolveError::ModelAmbiguous { candidates, .. }) => {
                    Err(MigrationAction::NeedsUserChoice { candidates })
                }
                Err(err) => Err(MigrationAction::NeedsResolution {
                    reason: err.to_string(),
                }),
            }
        }
        None => {
            if catalog.default_bindings.contains_key(capability_id) {
                Ok((
                    NodeSemanticsEntry {
                        capability_id: capability_id.to_owned(),
                        mode: capability_id.to_owned(),
                        implementation: ImplementationSelection::Policy {
                            policy_id: "capability_default".to_owned(),
                            constraints: Value::Object(serde_json::Map::new()),
                        },
                    },
                    MigrationAction::MappedPolicy {
                        capability_id: capability_id.to_owned(),
                    },
                ))
            } else {
                Err(MigrationAction::NeedsResolution {
                    reason: format!(
                        "no declared model and capability `{capability_id}` has no configured default binding"
                    ),
                })
            }
        }
    }
}

#[cfg(test)]
#[path = "semantics_tests.rs"]
mod tests;
