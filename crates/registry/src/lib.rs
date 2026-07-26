use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod catalog;
pub mod catalog_seed;
pub mod resolver;

mod builtin;
mod error;
mod types;

pub use builtin::builtin_node_definitions;
pub(crate) use builtin::{port, schema};
pub use error::RegistryError;
pub use types::{
    EstimatedCostRef, NodeDefinition, ParamSpec, ParamValueType, ParamsSchema, PortDefinition,
    PortType,
};

pub fn module_name() -> &'static str {
    "registry"
}

pub type RegistryResult<T> = Result<T, RegistryError>;

#[derive(Debug, Clone)]
pub struct NodeRegistry {
    definitions: BTreeMap<String, NodeDefinition>,
}

impl NodeRegistry {
    pub fn builtin() -> Self {
        Self::from_definitions(builtin_node_definitions())
            .expect("built-in node definitions are valid")
    }

    pub fn from_definitions(definitions: Vec<NodeDefinition>) -> RegistryResult<Self> {
        let mut by_type = BTreeMap::new();

        for definition in definitions {
            let node_type = definition.node_type.clone();
            if by_type.insert(node_type.clone(), definition).is_some() {
                return Err(RegistryError::DuplicateNodeType(node_type));
            }
        }

        Ok(Self {
            definitions: by_type,
        })
    }

    pub fn definition(&self, node_type: &str) -> RegistryResult<&NodeDefinition> {
        self.definitions
            .get(node_type)
            .ok_or_else(|| RegistryError::UnknownNodeType(node_type.to_owned()))
    }

    pub fn definitions(&self) -> impl Iterator<Item = &NodeDefinition> {
        self.definitions.values()
    }

    pub fn export_catalog(&self) -> RegistryCatalog {
        RegistryCatalog {
            schema_version: 1,
            nodes: self.definitions.values().cloned().collect(),
        }
    }

    pub fn validate_node_params(&self, node_type: &str, params: &Value) -> RegistryResult<()> {
        let definition = self.definition(node_type)?;
        definition.validate_params(params)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegistryCatalog {
    pub schema_version: u32,
    pub nodes: Vec<NodeDefinition>,
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
