use helixflow_graph::WorkflowGraph;
use helixflow_store::{ArtifactRecord, CostLedgerRecord, RunRecord, RunStepRecord};
use serde::{Deserialize, Serialize};

use super::RunOutcome;

#[derive(Debug, Clone)]
pub struct AgentRunRequest {
    pub workspace_id: String,
    pub version_id: String,
    pub group_id: Option<String>,
    pub label: String,
    pub provider: String,
    pub graph: WorkflowGraph,
}

#[derive(Debug, Clone)]
pub struct SweepPlan {
    pub workspace_id: String,
    pub version_id: String,
    pub label: String,
    pub provider: String,
    pub variants: Vec<SweepVariant>,
}

#[derive(Debug, Clone)]
pub struct SweepVariant {
    pub label: String,
    pub graph: WorkflowGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CostSummary {
    pub amount: f64,
    pub currency: String,
    pub estimated: bool,
    /// True when any contributing provider could not produce a trustworthy
    /// amount. Unknown totals always require explicit confirmation (HF-004).
    #[serde(default)]
    pub unknown: bool,
}

#[derive(Debug, Clone)]
pub struct PendingRun {
    pub run: RunRecord,
    pub steps: Vec<RunStepRecord>,
    pub estimate: CostSummary,
    pub ledger: Vec<CostLedgerRecord>,
}

#[derive(Debug, Clone)]
pub struct PendingSweep {
    pub group_id: String,
    pub runs: Vec<PendingRun>,
    pub estimate: CostSummary,
}

#[derive(Debug, Clone)]
pub struct SweepOutcome {
    pub group_id: String,
    pub runs: Vec<RunOutcome>,
    pub artifacts: Vec<ArtifactRecord>,
    pub recommendation: Option<ArtifactRecord>,
}
