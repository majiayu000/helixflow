//! Capability-driven workflow compiler (GH130 T3).
//!
//! Turns a validated [`IntentPlan`] into a typed graph, a semantic layer,
//! and a minimal proposal diff — deterministically. The Agent never emits
//! node ids, edges, coordinates, binding ids, or backend payloads; those are
//! all products of this crate (tech.md §6).

use std::collections::BTreeMap;

use helixflow_graph::graph_v2::{NodeSemanticsEntry, WorkflowGraphV2};
use helixflow_graph::{GraphService, ProposalOp, WorkflowGraph};
use helixflow_registry::catalog::CatalogSnapshot;
use helixflow_registry::resolver::ConnectorAvailability;
use serde::{Deserialize, Serialize};

pub mod errors;
mod graph_builder;
pub mod intent;
mod proposal_diff;

pub use errors::{ClarifyFirst, CompileError, CompileOutcome};
pub use intent::{IntentPlan, StageInputRef, StageIntent, TopologyIntent};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompiledProposal {
    pub graph_schema_version: u32,
    pub catalog_revision: String,
    pub ops: Vec<ProposalOp>,
    pub resolved_stages: Vec<ResolvedStage>,
    pub semantics: BTreeMap<String, NodeSemanticsEntry>,
    pub target: WorkflowGraphV2,
    pub layout_hints: Vec<LayoutHint>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolvedStage {
    pub stage_id: String,
    pub node_id: String,
    pub capability_id: String,
    pub requested_model_id: Option<String>,
    pub resolved_model_id: String,
    pub binding_id: String,
    pub binding_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutHint {
    pub node_id: String,
    pub pos: [f32; 2],
}

/// Compiles an intent against the current graph. `current` is the durable
/// structural graph the proposal will converge toward the compiled target
/// (an empty graph compiles to pure creation ops).
pub fn compile(
    intent: &IntentPlan,
    current: &WorkflowGraph,
    service: &GraphService,
    catalog: &CatalogSnapshot,
    availability: &ConnectorAvailability,
) -> Result<CompileOutcome, CompileError> {
    intent.validate()?;
    let built = match graph_builder::build(intent, service, catalog, availability)? {
        Ok(built) => built,
        Err(clarify) => return Ok(CompileOutcome::Clarify(clarify)),
    };

    let ops = proposal_diff::diff(current, &built.target.base);
    Ok(CompileOutcome::Compiled(CompiledProposal {
        graph_schema_version: built.target.schema_version,
        catalog_revision: built.target.catalog_revision.clone(),
        ops,
        resolved_stages: built.resolved_stages,
        semantics: built.target.semantics.clone(),
        target: built.target,
        layout_hints: built.layout_hints,
        diagnostics: built.diagnostics,
    }))
}

#[cfg(test)]
#[path = "compile_tests.rs"]
mod tests;
