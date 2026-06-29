use helixflow_graph::{PreparedProposal, ProposalKind, ProposalOp, ProposalState, WorkflowGraph};
use helixflow_run::{PendingRun, PendingSweep, RunOutcome};
use helixflow_store::{ArtifactRecord, CostLedgerRecord, ProposalRecord, RunRecord, RunStepRecord};
use serde::{Serialize, Serializer};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunPayload {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) status: String,
    pub(crate) steps: Vec<RunStepPayload>,
    pub(crate) cost: RunCostPayload,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunStepPayload {
    pub(crate) node_id: String,
    pub(crate) title: String,
    pub(crate) state: String,
    pub(crate) provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct RunCostPayload {
    pub(crate) estimate: f64,
    pub(crate) actual: f64,
    pub(crate) currency: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct OutputPayload {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) title: String,
    #[serde(rename = "storageUri")]
    pub(crate) storage_uri: String,
    pub(crate) selected: bool,
    pub(crate) meta: String,
    pub(crate) mime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) preview: Option<OutputPreviewPayload>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct OutputPreviewPayload {
    pub(crate) kind: OutputPreviewKind,
    pub(crate) content: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum OutputPreviewKind {
    Html,
    Text,
}

impl Serialize for OutputPreviewKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match self {
            Self::Html => "html",
            Self::Text => "text",
        })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingConfirmationPayload {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) summary: String,
    pub(crate) cost: ConfirmationCostPayload,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) run_count: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) pending_changes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) interruptible: Option<bool>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct ConfirmationCostPayload {
    pub(crate) amount: f64,
    pub(crate) currency: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProposalPayload {
    pub(crate) id: String,
    pub(crate) base_version_id: String,
    pub(crate) kind: ProposalKind,
    pub(crate) title: String,
    pub(crate) summary: String,
    pub(crate) ops: Vec<ProposalOp>,
    pub(crate) diff_summary: Vec<String>,
    pub(crate) preview_graph: WorkflowGraph,
    pub(crate) state: ProposalState,
    pub(crate) message_id: Option<String>,
}

pub(crate) fn run_payload_from_pending(pending: &PendingRun) -> RunPayload {
    RunPayload {
        id: pending.run.id.clone(),
        label: pending.run.label.clone(),
        status: pending.run.status.clone(),
        steps: run_step_payloads(&pending.steps),
        cost: RunCostPayload {
            estimate: pending.estimate.amount,
            actual: 0.0,
            currency: pending.estimate.currency.clone(),
        },
    }
}

pub(crate) fn run_payload_from_outcome(
    outcome: &RunOutcome,
    costs: &[CostLedgerRecord],
) -> RunPayload {
    RunPayload {
        id: outcome.run.id.clone(),
        label: outcome.run.label.clone(),
        status: outcome.run.status.clone(),
        steps: run_step_payloads(&outcome.steps),
        cost: run_cost_from_ledger(costs),
    }
}

pub(crate) fn pending_confirmation_from_pending(
    pending: &PendingRun,
) -> PendingConfirmationPayload {
    PendingConfirmationPayload {
        id: pending.run.id.clone(),
        title: pending.run.label.clone(),
        summary: "Run is waiting for confirmation".to_owned(),
        cost: ConfirmationCostPayload {
            amount: pending.estimate.amount,
            currency: pending.estimate.currency.clone(),
        },
        run_count: None,
        pending_changes: Vec::new(),
        interruptible: None,
    }
}

pub(crate) fn pending_confirmation_from_sweep(
    pending: &PendingSweep,
    recommended_run_id: &str,
    pending_changes: Vec<String>,
) -> PendingConfirmationPayload {
    PendingConfirmationPayload {
        id: recommended_run_id.to_owned(),
        title: "Seed sweep run plan".to_owned(),
        summary: format!(
            "Seed sweep is waiting for confirmation ({} runs).",
            pending.runs.len()
        ),
        cost: ConfirmationCostPayload {
            amount: pending.estimate.amount,
            currency: pending.estimate.currency.clone(),
        },
        run_count: Some(pending.runs.len()),
        pending_changes,
        interruptible: Some(true),
    }
}

pub(crate) fn pending_confirmation_from_run(
    run: &RunRecord,
    costs: &[CostLedgerRecord],
) -> Option<PendingConfirmationPayload> {
    if run.status != "waiting_confirmation" {
        return None;
    }
    let cost = run_cost_from_ledger(costs);
    Some(PendingConfirmationPayload {
        id: run.id.clone(),
        title: run.label.clone(),
        summary: "Run is waiting for confirmation".to_owned(),
        cost: ConfirmationCostPayload {
            amount: if cost.estimate > 0.0 {
                cost.estimate
            } else {
                cost.actual
            },
            currency: cost.currency,
        },
        run_count: None,
        pending_changes: Vec::new(),
        interruptible: None,
    })
}

pub(crate) fn output_payload_from_artifact(artifact: &ArtifactRecord) -> OutputPayload {
    let title = artifact
        .node_id
        .clone()
        .unwrap_or_else(|| artifact.kind.clone());
    OutputPayload {
        id: artifact.id.clone(),
        kind: artifact.kind.clone(),
        title: title.clone(),
        storage_uri: safe_download_uri(&artifact.id),
        selected: artifact.selected,
        meta: output_meta_summary(artifact),
        mime: artifact.mime.clone(),
        preview: output_preview_from_artifact(artifact, &title),
    }
}

pub(crate) fn safe_download_uri(artifact_id: &str) -> String {
    format!("/api/outputs/{artifact_id}/download")
}

pub(crate) fn output_preview_from_artifact(
    artifact: &ArtifactRecord,
    title: &str,
) -> Option<OutputPreviewPayload> {
    let meta = artifact
        .meta_json
        .as_deref()
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .unwrap_or_else(|| json!({}));
    let mut lines = vec![
        format!("Artifact: {title}"),
        format!("Kind: {}", artifact.kind),
    ];
    if let Some(mime) = &artifact.mime {
        lines.push(format!("MIME: {mime}"));
    }
    if let (Some(width), Some(height)) = (artifact.width, artifact.height) {
        lines.push(format!("Size: {width} x {height}"));
    }
    if let Some(duration_ms) = artifact.duration_ms {
        lines.push(format!("Duration: {:.2}s", duration_ms as f64 / 1000.0));
    }
    if let Some(provider) = safe_meta_string(&meta, "provider") {
        lines.push(format!("Provider: {provider}"));
    }
    if let Some(capability) = safe_meta_string(&meta, "capability") {
        lines.push(format!("Capability: {capability}"));
    }
    lines.push(format!("Download: {}", safe_download_uri(&artifact.id)));

    match artifact.kind.as_str() {
        "html" => Some(OutputPreviewPayload {
            kind: OutputPreviewKind::Html,
            content: format!(
                "<!doctype html><meta charset=\"utf-8\"><pre>{}</pre>",
                html_escape(&lines.join("\n"))
            ),
        }),
        "text" | "json" | "markdown" | "image" | "video" => Some(OutputPreviewPayload {
            kind: OutputPreviewKind::Text,
            content: lines.join("\n"),
        }),
        _ => None,
    }
}

fn output_meta_summary(artifact: &ArtifactRecord) -> String {
    let meta = artifact
        .meta_json
        .as_deref()
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .unwrap_or_else(|| json!({}));
    let mut parts = Vec::new();
    if let Some(mime) = &artifact.mime {
        parts.push(mime.clone());
    }
    if let (Some(width), Some(height)) = (artifact.width, artifact.height) {
        parts.push(format!("{width} x {height}"));
    }
    if let Some(duration_ms) = artifact.duration_ms {
        parts.push(format!("{:.2}s", duration_ms as f64 / 1000.0));
    }
    if let Some(provider) = safe_meta_string(&meta, "provider") {
        parts.push(format!("provider={provider}"));
    }
    if let Some(capability) = safe_meta_string(&meta, "capability") {
        parts.push(format!("capability={capability}"));
    }
    parts.join(" · ")
}

fn safe_meta_string(meta: &Value, key: &str) -> Option<String> {
    let value = meta.get(key).and_then(Value::as_str)?.trim();
    if value.is_empty()
        || value.len() > 80
        || value.contains('\n')
        || value.contains('\r')
        || value.contains("://")
        || value.contains("/Users/")
        || value.contains("/tmp/")
        || value.contains('\\')
        || value.starts_with('/')
        || looks_like_windows_absolute_path(value)
    {
        return None;
    }
    Some(value.to_owned())
}

fn looks_like_windows_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub(crate) fn proposal_payload_from_prepared(
    id: impl Into<String>,
    proposal: &PreparedProposal,
) -> ProposalPayload {
    ProposalPayload {
        id: id.into(),
        base_version_id: proposal.base_version_id.clone(),
        kind: proposal.kind,
        title: proposal.title.clone(),
        summary: proposal.summary.clone(),
        ops: proposal.ops.clone(),
        diff_summary: proposal.diff_summary.clone(),
        preview_graph: proposal.preview_graph.clone(),
        state: proposal.state,
        message_id: proposal.message_id.clone(),
    }
}

pub(crate) fn pending_proposal_payload_from_record(
    record: &ProposalRecord,
    ops: Vec<ProposalOp>,
    preview_graph: WorkflowGraph,
) -> Result<ProposalPayload, String> {
    Ok(ProposalPayload {
        id: record.id.clone(),
        base_version_id: record.base_version_id.clone(),
        kind: proposal_kind_from_str(&record.kind)?,
        title: record.title.clone(),
        summary: record.summary.clone(),
        ops,
        diff_summary: Vec::new(),
        preview_graph,
        state: proposal_state_from_str(&record.state)?,
        message_id: record.message_id.clone(),
    })
}

fn run_step_payloads(steps: &[RunStepRecord]) -> Vec<RunStepPayload> {
    steps
        .iter()
        .map(|step| RunStepPayload {
            node_id: step.node_id.clone(),
            title: step.node_id.clone(),
            state: step.state.clone(),
            provider: step.provider.clone(),
        })
        .collect()
}

fn run_cost_from_ledger(costs: &[CostLedgerRecord]) -> RunCostPayload {
    let mut cost = RunCostPayload {
        estimate: 0.0,
        actual: 0.0,
        currency: "USD".to_owned(),
    };
    for entry in costs {
        if cost.currency == "USD" {
            cost.currency = entry.currency.clone();
        }
        if entry.estimated {
            cost.estimate += entry.amount;
        } else {
            cost.actual += entry.amount;
        }
    }
    cost
}

pub(crate) fn proposal_kind_from_str(value: &str) -> Result<ProposalKind, String> {
    match value {
        "create" => Ok(ProposalKind::Create),
        "modify" => Ok(ProposalKind::Modify),
        "fix" => Ok(ProposalKind::Fix),
        "sweep" => Ok(ProposalKind::Sweep),
        _ => Err(format!("unknown proposal kind `{value}`")),
    }
}

fn proposal_state_from_str(value: &str) -> Result<ProposalState, String> {
    match value {
        "pending" => Ok(ProposalState::Pending),
        "applied" => Ok(ProposalState::Applied),
        "dismissed" => Ok(ProposalState::Dismissed),
        "superseded" => Ok(ProposalState::Superseded),
        "failed" => Ok(ProposalState::Invalid),
        _ => Err(format!("unknown proposal state `{value}`")),
    }
}
