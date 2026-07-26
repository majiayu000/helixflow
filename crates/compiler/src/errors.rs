//! Compile outcomes: hard errors and the clarify-first handoff (tech.md §4/§13).

use helixflow_registry::resolver::ResolveError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum CompileError {
    IntentInvalid {
        reason: String,
    },
    Topology {
        reason: String,
    },
    Resolve(ResolveError),
    PortTypeMismatch {
        stage_id: String,
        from_stage: String,
        output: String,
        reason: String,
    },
    UnmappedCapability {
        capability_id: String,
    },
    GraphInvalid {
        code: &'static str,
        message: String,
    },
}

impl CompileError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::IntentInvalid { .. } => "INTENT_INVALID",
            Self::Topology { .. } => "TOPOLOGY_CONFLICT",
            Self::Resolve(err) => err.code(),
            Self::PortTypeMismatch { .. } => "PORT_TYPE_MISMATCH",
            Self::UnmappedCapability { .. } => "CAPABILITY_NOT_FOUND",
            Self::GraphInvalid { code, .. } => code,
        }
    }
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IntentInvalid { reason } => write!(f, "invalid intent: {reason}"),
            Self::Topology { reason } => write!(f, "topology conflict: {reason}"),
            Self::Resolve(err) => write!(f, "{err}"),
            Self::PortTypeMismatch {
                stage_id,
                from_stage,
                output,
                reason,
            } => write!(
                f,
                "stage `{stage_id}` cannot consume `{from_stage}.{output}`: {reason}"
            ),
            Self::UnmappedCapability { capability_id } => write!(
                f,
                "capability `{capability_id}` has no executable node mapping"
            ),
            Self::GraphInvalid { message, .. } => {
                write!(f, "compiled graph failed validation: {message}")
            }
        }
    }
}

impl std::error::Error for CompileError {}

impl From<ResolveError> for CompileError {
    fn from(err: ResolveError) -> Self {
        Self::Resolve(err)
    }
}

/// The shared clarify handoff (`routing-contract.md`): returned instead of a
/// proposal whenever the intent cannot compile without another user decision.
/// Never presented as a successful proposal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClarifyFirst {
    pub route: String,
    pub reason_code: String,
    pub missing_fields: Vec<String>,
    pub safe_context: Value,
    pub next_action: String,
}

impl ClarifyFirst {
    pub fn new(
        reason_code: &str,
        missing_fields: Vec<String>,
        safe_context: Value,
        next_action: impl Into<String>,
    ) -> Self {
        Self {
            route: "clarify_first".to_owned(),
            reason_code: reason_code.to_owned(),
            missing_fields,
            safe_context,
            next_action: next_action.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompileOutcome {
    Compiled(crate::CompiledProposal),
    Clarify(ClarifyFirst),
}
