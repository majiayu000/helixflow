//! IntentPlan: the high-level agent contract (GH130 T3, tech.md §4).
//!
//! The Agent expresses stages, capabilities, requested models, input wiring,
//! and topology — never node ids, edge ids, coordinates, binding ids, or
//! backend payloads. Everything below this contract is compiled
//! deterministically.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::CompileError;

pub const INTENT_VERSION: &str = "1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IntentPlan {
    pub intent_version: String,
    pub topology: TopologyIntent,
    pub stages: Vec<StageIntent>,
    pub output_stage_ids: Vec<String>,
    #[serde(default)]
    pub assumptions: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TopologyIntent {
    Linear,
    Parallel,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageIntent {
    pub stage_id: String,
    pub capability_id: String,
    #[serde(default)]
    pub requested_model: Option<String>,
    #[serde(default)]
    pub input_from: Vec<StageInputRef>,
    #[serde(default = "empty_object")]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageInputRef {
    pub stage_id: String,
    pub output: String,
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

impl IntentPlan {
    /// Structural validation: versions, id shapes, reference order, and the
    /// declared topology. Runs before any catalog lookup.
    pub fn validate(&self) -> Result<(), CompileError> {
        if self.intent_version != INTENT_VERSION {
            return Err(CompileError::IntentInvalid {
                reason: format!("unsupported intentVersion `{}`", self.intent_version),
            });
        }
        if self.stages.is_empty() {
            return Err(CompileError::IntentInvalid {
                reason: "intent has no stages".to_owned(),
            });
        }
        if self.output_stage_ids.is_empty() {
            return Err(CompileError::IntentInvalid {
                reason: "intent declares no output stages".to_owned(),
            });
        }

        let mut seen = Vec::new();
        for stage in &self.stages {
            if !valid_stage_id(&stage.stage_id) {
                return Err(CompileError::IntentInvalid {
                    reason: format!("invalid stageId `{}`", stage.stage_id),
                });
            }
            if seen.contains(&stage.stage_id.as_str()) {
                return Err(CompileError::IntentInvalid {
                    reason: format!("duplicate stageId `{}`", stage.stage_id),
                });
            }
            if !stage.params.is_object() {
                return Err(CompileError::IntentInvalid {
                    reason: format!("stage `{}` params must be an object", stage.stage_id),
                });
            }
            // References may only point at earlier stages: acyclic by
            // construction and deterministic to compile.
            for input in &stage.input_from {
                if !seen.contains(&input.stage_id.as_str()) {
                    return Err(CompileError::Topology {
                        reason: format!(
                            "stage `{}` references `{}` which is not an earlier stage",
                            stage.stage_id, input.stage_id
                        ),
                    });
                }
            }
            seen.push(stage.stage_id.as_str());
        }

        for output_id in &self.output_stage_ids {
            if !seen.contains(&output_id.as_str()) {
                return Err(CompileError::IntentInvalid {
                    reason: format!("outputStageIds references unknown stage `{output_id}`"),
                });
            }
        }

        self.validate_topology()
    }

    fn validate_topology(&self) -> Result<(), CompileError> {
        let referenced: Vec<&str> = self
            .stages
            .iter()
            .flat_map(|stage| stage.input_from.iter())
            .map(|input| input.stage_id.as_str())
            .collect();
        let sinks: Vec<&str> = self
            .stages
            .iter()
            .map(|stage| stage.stage_id.as_str())
            .filter(|stage_id| !referenced.contains(stage_id))
            .collect();
        let mut declared: Vec<&str> = self.output_stage_ids.iter().map(String::as_str).collect();
        declared.sort_unstable();
        declared.dedup();
        let mut actual = sinks.clone();
        actual.sort_unstable();
        if declared != actual {
            return Err(CompileError::Topology {
                reason: format!(
                    "outputStageIds {declared:?} do not match the terminal stages {actual:?}"
                ),
            });
        }

        if self.topology == TopologyIntent::Linear {
            // A linear intent must form exactly one semantic main chain:
            // stage N consumes only stage N-1, and only the last stage is an
            // output (P6 — linear never compiles into a fan-out).
            for (index, stage) in self.stages.iter().enumerate() {
                if index == 0 {
                    if !stage.input_from.is_empty() {
                        return Err(CompileError::Topology {
                            reason: format!(
                                "linear intent: first stage `{}` cannot consume another stage",
                                stage.stage_id
                            ),
                        });
                    }
                    continue;
                }
                let previous = self.stages[index - 1].stage_id.as_str();
                if stage.input_from.is_empty()
                    || stage
                        .input_from
                        .iter()
                        .any(|input| input.stage_id != previous)
                {
                    return Err(CompileError::Topology {
                        reason: format!(
                            "linear intent: stage `{}` must consume exactly the previous stage `{previous}`",
                            stage.stage_id
                        ),
                    });
                }
            }
            if sinks.len() != 1 {
                return Err(CompileError::Topology {
                    reason: format!("linear intent must end in exactly one stage, found {sinks:?}"),
                });
            }
        }
        Ok(())
    }
}

fn valid_stage_id(stage_id: &str) -> bool {
    let mut chars = stage_id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && stage_id.len() <= 64
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
}
