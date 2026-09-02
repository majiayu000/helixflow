//! Capability catalog domain model (GH130 T1).
//!
//! The catalog is the single source of truth for which capability/model
//! combinations are executable. Capabilities and models are many-to-many:
//! only an explicit [`CapabilityBinding`] makes a pair executable (P3), and
//! every consumer must reference a `catalog_revision` (P2).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{ParamsSchema, PortDefinition};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogSnapshot {
    pub catalog_revision: String,
    pub capabilities: Vec<CapabilityDefinition>,
    pub models: Vec<ModelDefinition>,
    pub bindings: Vec<CapabilityBinding>,
    pub connectors: Vec<ConnectorDefinition>,
    pub workflow_backends: Vec<WorkflowBackendDefinition>,
    /// Explicitly configured policy defaults: capability id -> binding id.
    /// Policy selection may only use these entries (P5); pinned selection uses
    /// them solely to break ties between bindings of the *same* model.
    pub default_bindings: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityDefinition {
    pub capability_id: String,
    /// Stable executable node type for this capability. The runtime registry
    /// projects executable capabilities from this canonical definition.
    pub node_type: String,
    pub category: MediaCategory,
    pub display_name: String,
    pub description: String,
    pub inputs: Vec<PortDefinition>,
    pub outputs: Vec<PortDefinition>,
    pub params_schema: ParamsSchema,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MediaCategory {
    Text,
    Image,
    Video,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelDefinition {
    pub model_id: String,
    pub family_id: String,
    pub display_name: String,
    pub vendor: String,
    pub lifecycle: ModelLifecycle,
    /// Exact user-facing names this model may be requested by. Resolution is
    /// normalized exact-match only — never substring or "closest model".
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ModelLifecycle {
    Active,
    Deprecated,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectorDefinition {
    pub connector_id: String,
    pub provider_id: String,
    pub kind: ConnectorKind,
    pub enabled: bool,
    pub status: ConnectorStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConnectorKind {
    Api,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConnectorStatus {
    Active,
    Disabled,
}

/// Abstraction slot only in V1: no workflow backend ships with GH130 (the
/// ComfyUI backend was moved to a future issue), but the domain model keeps
/// the shape so backends can be added as catalog data later (P10).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowBackendDefinition {
    pub backend_id: String,
    pub kind: WorkflowBackendKind,
    pub enabled: bool,
    pub status: BackendStatus,
    pub catalog_revision: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowBackendKind {
    Comfyui,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BackendStatus {
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CapabilityBinding {
    pub binding_id: String,
    pub capability_id: String,
    pub model_id: String,
    pub implementation: ImplementationTarget,
    pub mode: String,
    pub input_schema: ParamsSchema,
    pub output_schema: ParamsSchema,
    pub defaults: serde_json::Value,
    pub availability: BindingAvailability,
    pub binding_revision: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BindingAvailability {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum ImplementationTarget {
    #[serde(rename_all = "camelCase")]
    ApiConnector {
        connector_id: String,
        operation_id: String,
    },
    #[serde(rename_all = "camelCase")]
    WorkflowTemplate {
        backend_id: String,
        template_id: String,
        template_revision: String,
    },
}

/// How a graph node selects its implementation. `Pinned` records the user's
/// explicit model choice and must resolve to exactly that model (P4);
/// `Policy` defers to the explicitly configured capability default (P5).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum ImplementationSelection {
    #[serde(rename_all = "camelCase")]
    Pinned {
        requested_model_id: String,
        binding_id: String,
    },
    #[serde(rename_all = "camelCase")]
    Policy {
        policy_id: String,
        #[serde(default)]
        constraints: serde_json::Value,
    },
}

impl CatalogSnapshot {
    /// Builds a snapshot with entries sorted by stable id and the revision
    /// computed over the sorted content, so identical data always produces an
    /// identical `catalog_revision`.
    pub fn build(
        mut capabilities: Vec<CapabilityDefinition>,
        mut models: Vec<ModelDefinition>,
        mut bindings: Vec<CapabilityBinding>,
        mut connectors: Vec<ConnectorDefinition>,
        mut workflow_backends: Vec<WorkflowBackendDefinition>,
        default_bindings: BTreeMap<String, String>,
    ) -> Self {
        capabilities.sort_by(|a, b| a.capability_id.cmp(&b.capability_id));
        models.sort_by(|a, b| a.model_id.cmp(&b.model_id));
        bindings.sort_by(|a, b| a.binding_id.cmp(&b.binding_id));
        connectors.sort_by(|a, b| a.connector_id.cmp(&b.connector_id));
        workflow_backends.sort_by(|a, b| a.backend_id.cmp(&b.backend_id));

        let mut snapshot = Self {
            catalog_revision: String::new(),
            capabilities,
            models,
            bindings,
            connectors,
            workflow_backends,
            default_bindings,
        };
        snapshot.catalog_revision = snapshot.compute_revision();
        snapshot
    }

    fn compute_revision(&self) -> String {
        use sha2::{Digest, Sha256};

        let mut content = self.clone();
        content.catalog_revision = String::new();
        let bytes = serde_json::to_vec(&content).expect("catalog serializes");
        let digest = Sha256::digest(&bytes);
        format!("sha256:{}", hex::encode(digest))
    }

    pub fn capability(&self, capability_id: &str) -> Option<&CapabilityDefinition> {
        self.capabilities
            .iter()
            .find(|capability| capability.capability_id == capability_id)
    }

    pub fn model(&self, model_id: &str) -> Option<&ModelDefinition> {
        self.models.iter().find(|model| model.model_id == model_id)
    }

    pub fn binding(&self, binding_id: &str) -> Option<&CapabilityBinding> {
        self.bindings
            .iter()
            .find(|binding| binding.binding_id == binding_id)
    }

    pub fn connector(&self, connector_id: &str) -> Option<&ConnectorDefinition> {
        self.connectors
            .iter()
            .find(|connector| connector.connector_id == connector_id)
    }

    pub fn bindings_for_capability(&self, capability_id: &str) -> Vec<&CapabilityBinding> {
        self.bindings
            .iter()
            .filter(|binding| binding.capability_id == capability_id)
            .collect()
    }

    pub fn bindings_for_pair(
        &self,
        capability_id: &str,
        model_id: &str,
    ) -> Vec<&CapabilityBinding> {
        self.bindings
            .iter()
            .filter(|binding| {
                binding.capability_id == capability_id && binding.model_id == model_id
            })
            .collect()
    }

    pub fn models_for_capability(&self, capability_id: &str) -> Vec<&ModelDefinition> {
        let mut model_ids: Vec<&str> = self
            .bindings_for_capability(capability_id)
            .into_iter()
            .map(|binding| binding.model_id.as_str())
            .collect();
        model_ids.sort_unstable();
        model_ids.dedup();
        model_ids
            .into_iter()
            .filter_map(|model_id| self.model(model_id))
            .collect()
    }

    pub fn capabilities_for_model(&self, model_id: &str) -> Vec<&CapabilityDefinition> {
        let mut capability_ids: Vec<&str> = self
            .bindings
            .iter()
            .filter(|binding| binding.model_id == model_id)
            .map(|binding| binding.capability_id.as_str())
            .collect();
        capability_ids.sort_unstable();
        capability_ids.dedup();
        capability_ids
            .into_iter()
            .filter_map(|capability_id| self.capability(capability_id))
            .collect()
    }
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
