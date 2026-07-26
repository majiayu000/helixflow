//! Registry error surface (#147 split; behavior unchanged).

use std::fmt;

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
