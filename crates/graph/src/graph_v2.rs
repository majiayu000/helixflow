//! Graph schema V2: semantic layer over the structural v1 graph (GH130 T2).
//!
//! V2 keeps the untouched v1 graph as its structural base and adds a
//! catalog-pinned semantic layer keyed by node id. Embedding the semantic
//! fields directly into `GraphNode` happens at SP130-T6 when the v1 write
//! path is deleted; until then the layered shape keeps every v1 consumer
//! working unchanged (decision recorded in `specs/GH130/tasks.md`).

use std::collections::BTreeMap;

use helixflow_registry::catalog::{CatalogSnapshot, ImplementationSelection};
use helixflow_registry::resolver::{
    CapabilityResolver, ConnectorAvailability, ResolveError, ResolveRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{GraphService, WorkflowGraph};

pub const GRAPH_SCHEMA_V2: u32 = 2;
pub const MIGRATION_VERSION: &str = "1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowGraphV2 {
    pub schema_version: u32,
    pub catalog_revision: String,
    /// Structural layer: nodes, edges, params, positions — byte-for-byte a
    /// valid v1 graph. Titles and positions never carry semantics (P1).
    pub base: WorkflowGraph,
    /// Semantic layer: one entry per executable node.
    pub semantics: BTreeMap<String, NodeSemanticsEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeSemanticsEntry {
    pub capability_id: String,
    pub mode: String,
    pub implementation: ImplementationSelection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphV2Error {
    UnsupportedSchema(u32),
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

impl GraphV2Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedSchema(_) => "UNSUPPORTED_SCHEMA",
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

impl std::fmt::Display for GraphV2Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchema(version) => {
                write!(f, "unsupported graph schema version: {version}")
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
                write!(
                    f,
                    "semantics entry for non-executable or unknown node `{node_id}`"
                )
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

impl std::error::Error for GraphV2Error {}

/// Maps a legacy registry capability id to its canonical V2 capability id.
/// `image_generate` was renamed to `text_to_image` with no runtime alias.
pub fn canonical_capability(legacy: &str) -> &str {
    match legacy {
        "image_generate" => "text_to_image",
        other => other,
    }
}

impl WorkflowGraphV2 {
    /// Validates the structural base and the semantic layer against a
    /// specific catalog revision. Fail-closed on every inconsistency (P8).
    pub fn validate(
        &self,
        service: &GraphService,
        catalog: &CatalogSnapshot,
    ) -> Result<(), GraphV2Error> {
        if self.schema_version != GRAPH_SCHEMA_V2 {
            return Err(GraphV2Error::UnsupportedSchema(self.schema_version));
        }
        if self.catalog_revision != catalog.catalog_revision {
            return Err(GraphV2Error::CatalogRevisionStale {
                graph_revision: self.catalog_revision.clone(),
                catalog_revision: catalog.catalog_revision.clone(),
            });
        }
        service
            .validate_graph(&self.base)
            .map_err(|err| GraphV2Error::StructuralInvalid(err.to_string()))?;

        let registry = service.registry();
        for (node_id, node) in &self.base.nodes {
            let definition = registry
                .definition(&node.node_type)
                .map_err(|err| GraphV2Error::StructuralInvalid(err.to_string()))?;
            match &definition.capability {
                Some(legacy_capability) => {
                    let Some(entry) = self.semantics.get(node_id) else {
                        return Err(GraphV2Error::MissingSemantics(node_id.clone()));
                    };
                    let wired: std::collections::BTreeSet<String> = self
                        .base
                        .edges
                        .iter()
                        .filter(|edge| edge.to[0] == *node_id)
                        .map(|edge| edge.to[1].clone())
                        .collect();
                    self.validate_entry(
                        node_id,
                        &node.node_type,
                        &node.params,
                        &wired,
                        canonical_capability(legacy_capability),
                        entry,
                        catalog,
                    )?;
                }
                None => {
                    if self.semantics.contains_key(node_id) {
                        return Err(GraphV2Error::UnexpectedSemantics(node_id.clone()));
                    }
                }
            }
        }
        for node_id in self.semantics.keys() {
            if !self.base.nodes.contains_key(node_id) {
                return Err(GraphV2Error::UnexpectedSemantics(node_id.clone()));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_entry(
        &self,
        node_id: &str,
        node_type: &str,
        params: &Value,
        wired: &std::collections::BTreeSet<String>,
        expected_capability: &str,
        entry: &NodeSemanticsEntry,
        catalog: &CatalogSnapshot,
    ) -> Result<(), GraphV2Error> {
        if entry.capability_id != expected_capability {
            return Err(GraphV2Error::CapabilityMismatch {
                node_id: node_id.to_owned(),
                node_type: node_type.to_owned(),
                capability_id: entry.capability_id.clone(),
            });
        }
        if catalog.capability(&entry.capability_id).is_none() {
            return Err(GraphV2Error::CapabilityNotFound {
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
                    return Err(GraphV2Error::BindingNotFound {
                        node_id: node_id.to_owned(),
                        binding_id: binding_id.clone(),
                    });
                };
                if binding.capability_id != entry.capability_id {
                    return Err(GraphV2Error::BindingMismatch {
                        node_id: node_id.to_owned(),
                        binding_id: binding_id.clone(),
                        reason: format!(
                            "binding implements `{}`, node declares `{}`",
                            binding.capability_id, entry.capability_id
                        ),
                    });
                }
                if &binding.model_id != requested_model_id {
                    return Err(GraphV2Error::PinnedModelMismatch {
                        node_id: node_id.to_owned(),
                        requested_model_id: requested_model_id.clone(),
                        binding_model_id: binding.model_id.clone(),
                    });
                }
                if binding.mode != entry.mode {
                    return Err(GraphV2Error::BindingMismatch {
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
                    .map_err(|err| GraphV2Error::ParamsInvalid {
                        node_id: node_id.to_owned(),
                        message: err.to_string(),
                    })?;
            }
            ImplementationSelection::Policy { .. } => {
                let Some(default_id) = catalog.default_bindings.get(&entry.capability_id) else {
                    return Err(GraphV2Error::PolicyDefaultMissing {
                        node_id: node_id.to_owned(),
                        capability_id: entry.capability_id.clone(),
                    });
                };
                if catalog.binding(default_id).is_none() {
                    return Err(GraphV2Error::BindingNotFound {
                        node_id: node_id.to_owned(),
                        binding_id: default_id.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationReport {
    pub migration_version: String,
    pub source_schema_version: u32,
    pub catalog_revision: String,
    pub workspace_connector_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<MigrationFailure>,
    pub nodes: Vec<NodeMigration>,
    pub resolvable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MigrationFailure {
    pub code: MigrationReasonCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MigrationReasonCode {
    UnknownNodeType,
    CapabilityNotFound,
    ModelNotFound,
    ModelAmbiguous,
    ModelCapabilityMismatch,
    BindingNotFound,
    BindingAmbiguous,
    DefaultBindingMissing,
    SourceGraphStructuralInvalid,
    MigratedGraphInvalid,
    WorkspaceConnectorIncompatible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationContext<'a> {
    pub workspace_connector_id: &'a str,
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
        code: MigrationReasonCode,
        message: String,
    },
    #[serde(rename_all = "camelCase")]
    NeedsUserChoice {
        code: MigrationReasonCode,
        message: String,
        candidates: Vec<String>,
    },
}

/// Migrates a v1 graph to the layered v2 shape. Deterministic and dry-run
/// friendly: the report is always produced; the graph is only produced when
/// every executable node is resolvable. Never infers a model from titles
/// (P14) and never adopts a provider-internal default: nodes without a
/// declared model become explicit `Policy` selections, which resolve only
/// through the configured default binding (P5).
pub fn migrate_v1_to_v2(
    graph: &WorkflowGraph,
    service: &GraphService,
    catalog: &CatalogSnapshot,
    context: MigrationContext<'_>,
) -> (Option<WorkflowGraphV2>, MigrationReport) {
    let validation_graph = legacy_validation_view(graph, service);
    if let Err(error) = service.validate_graph(&validation_graph) {
        return (
            None,
            failed_migration_report(
                graph,
                catalog,
                context,
                MigrationReasonCode::SourceGraphStructuralInvalid,
                format!("source graph validation failed: {error}"),
            ),
        );
    }

    let registry = service.registry();
    // The configured workspace connector is stable input. Runtime health is
    // deliberately excluded, but bindings from every other connector remain
    // unavailable so migration cannot pin an unusable implementation.
    let selected_available: ConnectorAvailability =
        BTreeMap::from([(context.workspace_connector_id.to_owned(), true)]);
    let resolver = CapabilityResolver::new(catalog);

    let mut base = graph.clone();
    let mut semantics = BTreeMap::new();
    let mut nodes = Vec::new();
    let mut resolvable = true;

    for (node_id, node) in &graph.nodes {
        let action = match registry.definition(&node.node_type) {
            Err(err) => {
                resolvable = false;
                MigrationAction::NeedsResolution {
                    code: MigrationReasonCode::UnknownNodeType,
                    message: format!("unknown node type: {err}"),
                }
            }
            Ok(definition) => match &definition.capability {
                None => MigrationAction::Structural,
                Some(legacy_capability) => {
                    let capability_id = canonical_capability(legacy_capability).to_owned();
                    let legacy_model = node
                        .params
                        .get("model")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    match migrate_executable_node(
                        &capability_id,
                        legacy_model.as_deref(),
                        &resolver,
                        &selected_available,
                        catalog,
                        context,
                    ) {
                        Ok((entry, action)) => {
                            if legacy_model.is_some()
                                && let Some(params) = base
                                    .nodes
                                    .get_mut(node_id)
                                    .and_then(|node| node.params.as_object_mut())
                            {
                                params.remove("model");
                            }
                            semantics.insert(node_id.clone(), entry);
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
        workspace_connector_id: context.workspace_connector_id.to_owned(),
        failure: None,
        nodes,
        resolvable,
    };
    let migrated = resolvable.then(|| WorkflowGraphV2 {
        schema_version: GRAPH_SCHEMA_V2,
        catalog_revision: catalog.catalog_revision.clone(),
        base,
        semantics,
    });
    match migrated {
        Some(candidate) => match candidate.validate(service, catalog) {
            Ok(()) => (Some(candidate), report),
            Err(error) => (
                None,
                failed_migration_report(
                    graph,
                    catalog,
                    context,
                    MigrationReasonCode::MigratedGraphInvalid,
                    format!("migrated graph validation failed: {error}"),
                ),
            ),
        },
        None => (None, report),
    }
}

fn legacy_validation_view(graph: &WorkflowGraph, service: &GraphService) -> WorkflowGraph {
    let mut validation_graph = graph.clone();
    for node in validation_graph.nodes.values_mut() {
        let is_executable = service
            .registry()
            .definition(&node.node_type)
            .ok()
            .and_then(|definition| definition.capability.as_ref())
            .is_some();
        if is_executable
            && node.params.get("model").is_some_and(Value::is_string)
            && let Some(params) = node.params.as_object_mut()
        {
            params.remove("model");
        }
    }
    validation_graph
}

fn migrate_executable_node(
    capability_id: &str,
    legacy_model: Option<&str>,
    resolver: &CapabilityResolver<'_>,
    selected_available: &ConnectorAvailability,
    catalog: &CatalogSnapshot,
    context: MigrationContext<'_>,
) -> Result<(NodeSemanticsEntry, MigrationAction), MigrationAction> {
    if catalog.capability(capability_id).is_none() {
        return Err(MigrationAction::NeedsResolution {
            code: MigrationReasonCode::CapabilityNotFound,
            message: format!("capability `{capability_id}` is not in the catalog"),
        });
    }

    match legacy_model {
        Some(model) => {
            let request = ResolveRequest {
                capability_id: capability_id.to_owned(),
                requested_model: Some(model.to_owned()),
                connector_preference: Some(context.workspace_connector_id.to_owned()),
            };
            match resolver.resolve(&request, selected_available) {
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
                Err(error) => Err(resolve_error_action(
                    error, &request, resolver, catalog, true,
                )),
            }
        }
        None => {
            if !catalog.default_bindings.contains_key(capability_id) {
                return Err(MigrationAction::NeedsResolution {
                    code: MigrationReasonCode::DefaultBindingMissing,
                    message: format!(
                        "capability `{capability_id}` has no configured default binding"
                    ),
                });
            }
            let request = ResolveRequest {
                capability_id: capability_id.to_owned(),
                requested_model: None,
                connector_preference: Some(context.workspace_connector_id.to_owned()),
            };
            match resolver.resolve(&request, selected_available) {
                Ok(_) => Ok((
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
                )),
                Err(error) => Err(resolve_error_action(
                    error, &request, resolver, catalog, false,
                )),
            }
        }
    }
}

fn resolve_error_action(
    error: ResolveError,
    request: &ResolveRequest,
    resolver: &CapabilityResolver<'_>,
    catalog: &CatalogSnapshot,
    pinned: bool,
) -> MigrationAction {
    if matches!(
        error,
        ResolveError::BindingNotFound { .. } | ResolveError::BindingUnavailable { .. }
    ) {
        let all_available: ConnectorAvailability = catalog
            .connectors
            .iter()
            .map(|connector| (connector.connector_id.clone(), true))
            .collect();
        let unrestricted = ResolveRequest {
            connector_preference: None,
            ..request.clone()
        };
        if resolver.resolve(&unrestricted, &all_available).is_ok() {
            return MigrationAction::NeedsResolution {
                code: MigrationReasonCode::WorkspaceConnectorIncompatible,
                message: "no compatible binding exists for the workspace connector".to_owned(),
            };
        }
    }

    match error {
        ResolveError::ModelAmbiguous { mut candidates, .. } => {
            candidates.sort();
            candidates.dedup();
            MigrationAction::NeedsUserChoice {
                code: MigrationReasonCode::ModelAmbiguous,
                message: "multiple catalog models match the declared model".to_owned(),
                candidates,
            }
        }
        ResolveError::CapabilityNotFound { .. } => MigrationAction::NeedsResolution {
            code: MigrationReasonCode::CapabilityNotFound,
            message: "node capability is not present in the catalog".to_owned(),
        },
        ResolveError::ModelNotFound { .. } => MigrationAction::NeedsResolution {
            code: MigrationReasonCode::ModelNotFound,
            message: "declared model is not present in the catalog".to_owned(),
        },
        ResolveError::BindingNotFound { .. } if pinned => MigrationAction::NeedsResolution {
            code: MigrationReasonCode::ModelCapabilityMismatch,
            message: "declared model does not implement the node capability".to_owned(),
        },
        ResolveError::BindingNotFound { .. } | ResolveError::BindingUnavailable { .. } => {
            MigrationAction::NeedsResolution {
                code: MigrationReasonCode::BindingNotFound,
                message: "no usable catalog binding implements the node capability".to_owned(),
            }
        }
        ResolveError::BindingAmbiguous { .. } => MigrationAction::NeedsResolution {
            code: MigrationReasonCode::BindingAmbiguous,
            message: "multiple catalog bindings match the node capability".to_owned(),
        },
    }
}

fn failed_migration_report(
    graph: &WorkflowGraph,
    catalog: &CatalogSnapshot,
    context: MigrationContext<'_>,
    code: MigrationReasonCode,
    message: String,
) -> MigrationReport {
    MigrationReport {
        migration_version: MIGRATION_VERSION.to_owned(),
        source_schema_version: graph.schema_version,
        catalog_revision: catalog.catalog_revision.clone(),
        workspace_connector_id: context.workspace_connector_id.to_owned(),
        failure: Some(MigrationFailure { code, message }),
        nodes: Vec::new(),
        resolvable: false,
    }
}

#[cfg(test)]
#[path = "graph_v2_tests.rs"]
mod tests;
