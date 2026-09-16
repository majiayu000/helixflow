//! Node, port, and params-schema types (#147 split; behavior unchanged).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::RegistryResult;
use crate::error::RegistryError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeDefinition {
    #[serde(rename = "type")]
    pub node_type: String,
    pub title: String,
    pub category: String,
    pub provider: Option<String>,
    pub capability: Option<String>,
    pub description: String,
    pub inputs: Vec<PortDefinition>,
    pub outputs: Vec<PortDefinition>,
    pub params_schema: ParamsSchema,
    pub estimated_cost: Option<EstimatedCostRef>,
}

impl NodeDefinition {
    pub(crate) fn validate_params(&self, params: &Value) -> RegistryResult<()> {
        self.params_schema.validate_value(&self.node_type, params)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PortDefinition {
    pub name: String,
    #[serde(rename = "type")]
    pub port_type: PortType,
    pub required: bool,
    /// `one` is the default and keeps the historical single-edge contract.
    /// `many` is a list: several same-type edges may land on this port.
    #[serde(default, skip_serializing_if = "is_one_cardinality")]
    pub cardinality: PortCardinality,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PortCardinality {
    #[default]
    One,
    Many,
}

impl PortCardinality {
    pub fn allows_fan_in(self) -> bool {
        matches!(self, Self::Many)
    }
}

fn is_one_cardinality(cardinality: &PortCardinality) -> bool {
    matches!(cardinality, PortCardinality::One)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum PortType {
    Text,
    Image,
    Video,
    Audio,
    Mask,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParamsSchema {
    pub required: Vec<String>,
    pub properties: BTreeMap<String, ParamSpec>,
    pub allow_unknown: bool,
}

impl ParamsSchema {
    /// Validates a params object against this schema. `context` names the
    /// owning node type or binding in error messages.
    pub fn validate_value(&self, context: &str, params: &Value) -> RegistryResult<()> {
        self.validate_value_with_wired(context, params, &BTreeSet::new())
    }

    /// Like [`Self::validate_value`], but a required param may also be
    /// satisfied by a wired input port named in `wired` instead of a literal
    /// param value.
    pub fn validate_value_with_wired(
        &self,
        context: &str,
        params: &Value,
        wired: &BTreeSet<String>,
    ) -> RegistryResult<()> {
        let Some(map) = params.as_object() else {
            return Err(RegistryError::ParamsNotObject(context.to_owned()));
        };

        for required in &self.required {
            if !map.contains_key(required) && !wired.contains(required) {
                return Err(RegistryError::MissingRequiredParam {
                    node_type: context.to_owned(),
                    param: required.clone(),
                });
            }
        }

        for (param, value) in map {
            let Some(spec) = self.properties.get(param) else {
                if self.allow_unknown {
                    continue;
                }

                return Err(RegistryError::UnknownParam {
                    node_type: context.to_owned(),
                    param: param.clone(),
                });
            };

            spec.validate(context, param, value)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParamSpec {
    #[serde(rename = "type")]
    pub value_type: ParamValueType,
    pub enum_values: Vec<Value>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
}

impl ParamSpec {
    pub fn string() -> Self {
        Self::new(ParamValueType::String)
    }

    pub fn integer() -> Self {
        Self::new(ParamValueType::Integer)
    }

    pub fn boolean() -> Self {
        Self::new(ParamValueType::Boolean)
    }

    pub fn integer_range(minimum: i64, maximum: i64) -> Self {
        Self {
            minimum: Some(minimum as f64),
            maximum: Some(maximum as f64),
            ..Self::integer()
        }
    }

    pub fn string_enum(values: &[&str]) -> Self {
        Self {
            enum_values: values
                .iter()
                .map(|value| Value::String((*value).to_owned()))
                .collect(),
            ..Self::string()
        }
    }

    fn new(value_type: ParamValueType) -> Self {
        Self {
            value_type,
            enum_values: Vec::new(),
            minimum: None,
            maximum: None,
        }
    }

    fn validate(&self, node_type: &str, param: &str, value: &Value) -> RegistryResult<()> {
        let valid_type = match self.value_type {
            ParamValueType::String => value.is_string(),
            ParamValueType::Integer => value.as_i64().is_some(),
            ParamValueType::Number => value.as_f64().is_some(),
            ParamValueType::Boolean => value.is_boolean(),
        };

        if !valid_type {
            return Err(RegistryError::InvalidParamType {
                node_type: node_type.to_owned(),
                param: param.to_owned(),
                expected: self.value_type.label(),
            });
        }

        if !self.enum_values.is_empty() && !self.enum_values.iter().any(|allowed| allowed == value)
        {
            return Err(RegistryError::ParamNotInEnum {
                node_type: node_type.to_owned(),
                param: param.to_owned(),
            });
        }

        if let Some(number) = value.as_f64() {
            let below_min = self.minimum.is_some_and(|minimum| number < minimum);
            let above_max = self.maximum.is_some_and(|maximum| number > maximum);
            if below_min || above_max {
                return Err(RegistryError::ParamOutOfRange {
                    node_type: node_type.to_owned(),
                    param: param.to_owned(),
                });
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParamValueType {
    String,
    Integer,
    Number,
    Boolean,
}

impl ParamValueType {
    fn label(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Boolean => "boolean",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EstimatedCostRef {
    pub unit: String,
    pub catalog_key: String,
}
