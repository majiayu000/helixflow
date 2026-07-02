use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub fn module_name() -> &'static str {
    "registry"
}

pub type RegistryResult<T> = Result<T, RegistryError>;

#[derive(Debug, Clone, PartialEq)]
pub enum RegistryError {
    DuplicateNodeType(String),
    UnknownNodeType(String),
    ParamsNotObject(String),
    MissingRequiredParam {
        node_type: String,
        param: String,
    },
    UnknownParam {
        node_type: String,
        param: String,
    },
    InvalidParamType {
        node_type: String,
        param: String,
        expected: &'static str,
    },
    ParamOutOfRange {
        node_type: String,
        param: String,
    },
    ParamNotInEnum {
        node_type: String,
        param: String,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateNodeType(node_type) => write!(f, "duplicate node type: {node_type}"),
            Self::UnknownNodeType(node_type) => write!(f, "unknown node type: {node_type}"),
            Self::ParamsNotObject(node_type) => {
                write!(f, "node params must be an object: {node_type}")
            }
            Self::MissingRequiredParam { node_type, param } => {
                write!(
                    f,
                    "missing required param `{param}` for node type `{node_type}`"
                )
            }
            Self::UnknownParam { node_type, param } => {
                write!(f, "unknown param `{param}` for node type `{node_type}`")
            }
            Self::InvalidParamType {
                node_type,
                param,
                expected,
            } => write!(
                f,
                "invalid param `{param}` for node type `{node_type}`; expected {expected}"
            ),
            Self::ParamOutOfRange { node_type, param } => {
                write!(
                    f,
                    "param `{param}` is out of range for node type `{node_type}`"
                )
            }
            Self::ParamNotInEnum { node_type, param } => {
                write!(
                    f,
                    "param `{param}` is not allowed for node type `{node_type}`"
                )
            }
        }
    }
}

impl std::error::Error for RegistryError {}

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
    fn validate_params(&self, params: &Value) -> RegistryResult<()> {
        let Some(map) = params.as_object() else {
            return Err(RegistryError::ParamsNotObject(self.node_type.clone()));
        };

        for required in &self.params_schema.required {
            if !map.contains_key(required) {
                return Err(RegistryError::MissingRequiredParam {
                    node_type: self.node_type.clone(),
                    param: required.clone(),
                });
            }
        }

        for (param, value) in map {
            let Some(spec) = self.params_schema.properties.get(param) else {
                if self.params_schema.allow_unknown {
                    continue;
                }

                return Err(RegistryError::UnknownParam {
                    node_type: self.node_type.clone(),
                    param: param.clone(),
                });
            };

            spec.validate(&self.node_type, param, value)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PortDefinition {
    pub name: String,
    #[serde(rename = "type")]
    pub port_type: PortType,
    pub required: bool,
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

pub fn builtin_node_definitions() -> Vec<NodeDefinition> {
    vec![
        node(
            "input.text",
            "Text Input",
            "input",
            None,
            None,
            "A user-provided text value.",
            vec![],
            vec![port("text", PortType::Text, true)],
            schema(&["text"], [("text", ParamSpec::string())]),
        ),
        node(
            "input.image",
            "Image Input",
            "input",
            None,
            None,
            "A user-uploaded image artifact.",
            vec![],
            vec![port("image", PortType::Image, true)],
            schema(&["storage_uri"], [("storage_uri", ParamSpec::string())]),
        ),
        node(
            "llm.prompt_writer",
            "Prompt Writer",
            "text",
            Some("atlas"),
            Some("chat_completion"),
            "Drafts text through the configured Atlas chat API provider.",
            vec![],
            vec![port("prompt", PortType::Text, true)],
            schema(
                &["prompt"],
                [
                    ("prompt", ParamSpec::string()),
                    (
                        "style",
                        ParamSpec::string_enum(&["cinematic", "product", "plain"]),
                    ),
                    ("model", ParamSpec::string()),
                ],
            ),
        ),
        node(
            "image.atlas.generate",
            "Atlas Image",
            "image",
            Some("atlas"),
            Some("image_generate"),
            "Generates an image through the configured Atlas API provider.",
            vec![],
            vec![port("image", PortType::Image, true)],
            schema(
                &["prompt", "aspect_ratio"],
                [
                    ("prompt", ParamSpec::string()),
                    (
                        "aspect_ratio",
                        ParamSpec::string_enum(&["1:1", "9:16", "16:9"]),
                    ),
                    ("seed", ParamSpec::integer()),
                ],
            ),
        ),
        node(
            "video.atlas.text_to_video",
            "Atlas Text To Video",
            "video",
            Some("atlas"),
            Some("text_to_video"),
            "Generates a video from prompt text through the configured Atlas API provider.",
            vec![],
            vec![port("video", PortType::Video, true)],
            schema(
                &["prompt", "duration_sec", "resolution"],
                [
                    ("prompt", ParamSpec::string()),
                    ("duration_sec", ParamSpec::integer_range(1, 15)),
                    (
                        "resolution",
                        ParamSpec::string_enum(&["480P", "720P", "1080P"]),
                    ),
                    ("model", ParamSpec::string()),
                ],
            ),
        ),
        node(
            "video.atlas.image_to_video",
            "Atlas Image To Video",
            "video",
            Some("atlas"),
            Some("image_to_video"),
            "Generates a video from an image URL or data URI through the configured Atlas API provider.",
            vec![port("image", PortType::Image, true)],
            vec![port("video", PortType::Video, true)],
            schema(
                &["prompt", "duration_sec", "resolution"],
                [
                    ("prompt", ParamSpec::string()),
                    ("duration_sec", ParamSpec::integer_range(1, 15)),
                    (
                        "resolution",
                        ParamSpec::string_enum(&["480P", "720P", "1080P"]),
                    ),
                    ("model", ParamSpec::string()),
                    ("image", ParamSpec::string()),
                ],
            ),
        ),
        node(
            "output.save",
            "Save Output",
            "output",
            None,
            None,
            "Marks an upstream artifact as a workflow output.",
            vec![port("artifact", PortType::Json, true)],
            vec![],
            schema::<0>(&[], []),
        ),
    ]
}

fn node(
    node_type: &str,
    title: &str,
    category: &str,
    provider: Option<&str>,
    capability: Option<&str>,
    description: &str,
    inputs: Vec<PortDefinition>,
    outputs: Vec<PortDefinition>,
    params_schema: ParamsSchema,
) -> NodeDefinition {
    NodeDefinition {
        node_type: node_type.to_owned(),
        title: title.to_owned(),
        category: category.to_owned(),
        provider: provider.map(str::to_owned),
        capability: capability.map(str::to_owned),
        description: description.to_owned(),
        inputs,
        outputs,
        params_schema,
        estimated_cost: provider
            .zip(capability)
            .map(|(provider, capability)| EstimatedCostRef {
                unit: "call".to_owned(),
                catalog_key: format!("{provider}.{capability}"),
            }),
    }
}

fn port(name: &str, port_type: PortType, required: bool) -> PortDefinition {
    PortDefinition {
        name: name.to_owned(),
        port_type,
        required,
    }
}

fn schema<const N: usize>(required: &[&str], properties: [(&str, ParamSpec); N]) -> ParamsSchema {
    ParamsSchema {
        required: required.iter().map(|value| (*value).to_owned()).collect(),
        properties: properties
            .into_iter()
            .map(|(name, spec)| (name.to_owned(), spec))
            .collect(),
        allow_unknown: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reports_module_name() {
        assert_eq!(module_name(), "registry");
    }

    #[test]
    fn serializes_node_definition_boundary() {
        let definition = NodeDefinition {
            node_type: "image.atlas.generate".to_string(),
            title: "Atlas Image".to_string(),
            category: "image".to_string(),
            provider: Some("atlas".to_string()),
            capability: Some("image_generate".to_string()),
            description: "An Atlas image node.".to_string(),
            inputs: Vec::new(),
            outputs: vec![PortDefinition {
                name: "image".to_string(),
                port_type: PortType::Image,
                required: true,
            }],
            params_schema: schema(&["prompt"], [("prompt", ParamSpec::string())]),
            estimated_cost: Some(EstimatedCostRef {
                unit: "call".to_string(),
                catalog_key: "mock.image_generate".to_string(),
            }),
        };

        let encoded = serde_json::to_value(&definition).expect("serialize node definition");

        assert_eq!(encoded["type"], "image.atlas.generate");
        assert_eq!(encoded["title"], "Atlas Image");
        assert_eq!(encoded["category"], "image");
        assert_eq!(encoded["provider"], "atlas");
    }

    #[test]
    fn rejects_unknown_node_types() {
        let registry = NodeRegistry::builtin();

        let err = registry
            .validate_node_params("video.missing", &json!({}))
            .expect_err("unknown node should fail");

        assert_eq!(
            err,
            RegistryError::UnknownNodeType("video.missing".to_owned())
        );
    }

    #[test]
    fn validates_required_params_and_schema() {
        let registry = NodeRegistry::builtin();

        registry
            .validate_node_params(
                "video.atlas.text_to_video",
                &json!({
                    "prompt": "A clean product ad",
                    "duration_sec": 5,
                    "resolution": "720P"
                }),
            )
            .expect("valid params");

        assert!(matches!(
            registry.validate_node_params(
                "video.atlas.text_to_video",
                &json!({ "prompt": "missing duration", "resolution": "720P" })
            ),
            Err(RegistryError::MissingRequiredParam { .. })
        ));
        assert!(matches!(
            registry.validate_node_params(
                "video.atlas.text_to_video",
                &json!({
                    "prompt": "bad duration",
                    "duration_sec": 30,
                    "resolution": "720P"
                })
            ),
            Err(RegistryError::ParamOutOfRange { .. })
        ));
    }

    #[test]
    fn exports_catalog_for_agent_context() {
        let registry = NodeRegistry::builtin();
        let catalog = registry.export_catalog();

        assert_eq!(catalog.schema_version, 1);
        assert!(
            catalog
                .nodes
                .iter()
                .any(|node| node.node_type == "video.atlas.text_to_video")
        );
    }
}
