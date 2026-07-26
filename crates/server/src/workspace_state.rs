use axum::{
    Json,
    extract::{Path as AxumPath, State},
};
use helixflow_agent::TurnMode;
use helixflow_graph::{ProposalOp, WorkflowGraph};
use helixflow_registry::NodeRegistry;
use helixflow_store::{
    ArtifactRecord, CostLedgerRecord, MessageRecord, ProposalRecord, RunRecord, RunStepRecord,
    VersionRecord, WorkspaceRecord,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::{blank_graph, read_graph_file, read_json_file};
use crate::version_file_consistency::read_version_graph;
use crate::workbench_payload::{
    ProposalPayload, output_payload_from_artifact, pending_confirmation_from_run,
    pending_proposal_payload_from_record,
};
use crate::workspace_state_run::visible_workspace_run;

pub(crate) async fn workspace_state(
    AxumPath(workspace_id): AxumPath<String>,
    State(state): State<AppState>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(workspace_state_value(&state, &workspace_id).await?))
}

pub(crate) async fn workspace_state_value(
    state: &AppState,
    workspace_id: &str,
) -> Result<Value, ApiError> {
    let workspace = state
        .store
        .workspace(workspace_id)
        .await
        .map_err(ApiError::store)?;
    let versions = state
        .store
        .versions_for_workspace(workspace_id)
        .await
        .map_err(ApiError::store)?;
    let current_version = current_version(&state, &workspace).await?;
    let graph = match &current_version {
        Some(version) => read_version_graph(&state.data_dir, version)
            .await
            .map_err(|error| ApiError::server_error(error.to_string()))?,
        None => blank_graph(),
    };
    let messages = state
        .store
        .workspace_messages(workspace_id)
        .await
        .map_err(ApiError::store)?;
    let proposals = state
        .store
        .workspace_proposals(workspace_id)
        .await
        .map_err(ApiError::store)?;
    let pending_proposal = pending_proposal_payload(
        &state,
        proposals.iter().rev().find(|item| item.state == "pending"),
    )
    .await?;
    let latest_run = state
        .store
        .latest_workspace_run(workspace_id)
        .await
        .map_err(ApiError::store)?;
    let latest_run = visible_workspace_run(state, latest_run).await?;
    let (steps, artifacts, costs, event_seq) = match &latest_run {
        Some(run) => {
            let steps = state
                .store
                .run_steps(&run.id)
                .await
                .map_err(ApiError::store)?;
            let artifacts = workspace_artifacts_for_run(state, run).await?;
            let costs = state
                .store
                .cost_ledger_for_run(&run.id)
                .await
                .map_err(ApiError::store)?;
            let event_seq = state
                .store
                .run_events(&run.id)
                .await
                .map_err(ApiError::store)?
                .last()
                .map(|event| event.seq)
                .unwrap_or(0);
            (steps, artifacts, costs, event_seq)
        }
        None => (Vec::new(), Vec::new(), Vec::new(), 0),
    };

    let providers = provider_state_value(state, &workspace)?;

    Ok(workspace_state_payload(
        &workspace,
        &graph,
        &messages,
        &proposals,
        pending_proposal,
        &versions,
        latest_run.as_ref(),
        &steps,
        &artifacts,
        &costs,
        event_seq,
        providers,
    ))
}

async fn workspace_artifacts_for_run(
    state: &AppState,
    run: &RunRecord,
) -> Result<Vec<ArtifactRecord>, ApiError> {
    if run.trigger == "sweep"
        && let Some(group_id) = run.group_id.as_deref()
    {
        return state
            .store
            .artifacts_for_group(group_id)
            .await
            .map_err(ApiError::store);
    }

    state
        .store
        .run_artifacts(&run.id)
        .await
        .map_err(ApiError::store)
}

async fn current_version(
    state: &AppState,
    workspace: &WorkspaceRecord,
) -> Result<Option<VersionRecord>, ApiError> {
    match workspace.cur_version_id.as_deref() {
        Some(version_id) => state
            .store
            .version(version_id)
            .await
            .map(Some)
            .map_err(ApiError::store),
        None => Ok(None),
    }
}

fn workspace_state_payload(
    workspace: &WorkspaceRecord,
    graph: &WorkflowGraph,
    messages: &[MessageRecord],
    proposals: &[ProposalRecord],
    pending_proposal: Option<ProposalPayload>,
    versions: &[VersionRecord],
    latest_run: Option<&RunRecord>,
    steps: &[RunStepRecord],
    artifacts: &[ArtifactRecord],
    costs: &[CostLedgerRecord],
    event_seq: i64,
    providers: Value,
) -> Value {
    let registry = NodeRegistry::builtin();
    let selected_provider = providers
        .get("selectedProvider")
        .and_then(Value::as_str)
        .unwrap_or("missing");
    let step_by_node = steps
        .iter()
        .map(|step| (step.node_id.as_str(), step))
        .collect::<BTreeMap<_, _>>();
    let run_cost = summarize_cost(costs);
    json!({
        "eventSeq": event_seq,
        "workspace": {
            "id": workspace.id,
            "name": workspace.name,
            "versionId": workspace.cur_version_id.clone().unwrap_or_default(),
            "updatedAt": workspace.updated_at,
        },
        "providers": providers,
        "chat": {
            "messages": messages.iter().map(chat_message_payload).collect::<Vec<_>>(),
        },
        "graph": graph_payload(graph, &step_by_node, &registry, selected_provider),
        "run": latest_run.map(|run| run_payload(run, steps, &run_cost)),
        "outputs": artifacts.iter().map(output_payload_from_artifact).collect::<Vec<_>>(),
        "history": history_payload(versions, latest_run, proposals),
        "pendingConfirmation": latest_run.and_then(|run| pending_confirmation_from_run(run, costs)),
        "pendingProposal": pending_proposal,
        "workflowGraph": graph,
    })
}

fn provider_state_value(state: &AppState, workspace: &WorkspaceRecord) -> Result<Value, ApiError> {
    let selected_provider = state.selected_provider_for_workspace(workspace);
    let mut providers = serde_json::to_value(state.provider_catalog_for_workspace(workspace))
        .map_err(|err| ApiError::server_error(err.to_string()))?;
    providers["selectedProvider"] = json!(selected_provider);
    Ok(providers)
}

async fn pending_proposal_payload(
    state: &AppState,
    proposal: Option<&ProposalRecord>,
) -> Result<Option<ProposalPayload>, ApiError> {
    let Some(proposal) = proposal else {
        return Ok(None);
    };
    let preview_path = proposal.preview_graph_path.as_deref().ok_or_else(|| {
        ApiError::server_error(format!(
            "pending proposal `{}` has no preview graph",
            proposal.id
        ))
    })?;
    let ops: Vec<ProposalOp> =
        read_json_file(&state.data_dir, &proposal.ops_path, "read proposal ops").await?;
    let preview_graph = read_graph_file(&state.data_dir, preview_path).await?;
    pending_proposal_payload_from_record(proposal, ops, preview_graph)
        .map(Some)
        .map_err(ApiError::server_error)
}

fn chat_message_payload(message: &MessageRecord) -> Value {
    let mut payload = json!({
        "id": message.id,
        "role": message.role,
        "kind": message.kind,
        "text": message.text.clone().unwrap_or_default(),
        "time": message.created_at,
    });
    if let Some(turn_mode) = message_turn_mode(message) {
        payload["turnMode"] = json!(turn_mode);
    }
    payload
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageMetadata {
    turn_mode: Option<TurnMode>,
}

fn message_turn_mode(message: &MessageRecord) -> Option<TurnMode> {
    message
        .attachment_ids_json
        .as_deref()
        .and_then(|value| serde_json::from_str::<MessageMetadata>(value).ok())
        .and_then(|metadata| metadata.turn_mode)
}

pub(crate) fn graph_payload(
    graph: &WorkflowGraph,
    step_by_node: &BTreeMap<&str, &RunStepRecord>,
    registry: &NodeRegistry,
    selected_provider: &str,
) -> Value {
    json!({
        "nodes": graph.nodes.iter().map(|(id, node)| {
            let step = step_by_node.get(id.as_str()).copied();
            json!({
                "id": id,
                "nodeType": node.node_type,
                "title": node.title,
                "category": node_category(&node.node_type),
                "status": step.map(|item| item.state.as_str()).unwrap_or("queued"),
                "cached": step.is_some_and(step_cached),
                "position": { "x": node.pos[0], "y": node.pos[1] },
                "provider": step.and_then(|item| item.provider.clone()).or_else(|| node_provider(&node.node_type, registry, selected_provider)),
                "summary": node_summary(&node.node_type, &node.params),
            })
        }).collect::<Vec<_>>(),
        "edges": graph.edges.iter().enumerate().map(|(idx, edge)| {
            json!({
                "id": format!(
                    "edge_{}_{}_{}_{}_{}",
                    edge.from[0], edge.from[1], edge.to[0], edge.to[1], idx
                ),
                "from": { "nodeId": edge.from[0], "port": edge.from[1] },
                "to": { "nodeId": edge.to[0], "port": edge.to[1] },
                "kind": edge.edge_type,
            })
        }).collect::<Vec<_>>(),
    })
}

fn node_provider(
    node_type: &str,
    registry: &NodeRegistry,
    selected_provider: &str,
) -> Option<String> {
    let definition = registry.definition(node_type).ok()?;
    if definition.capability.is_some() {
        return Some(selected_provider.to_owned());
    }
    definition.provider.clone()
}

fn run_payload(run: &RunRecord, steps: &[RunStepRecord], cost: &CostSummary) -> Value {
    json!({
        "id": run.id,
        "label": run.label,
        "status": run.status,
        "error": error_payload(run.error_json.as_deref()),
        "steps": steps.iter().map(|step| json!({
            "nodeId": step.node_id,
            "title": step.node_id,
            "state": step.state,
            "provider": step.provider,
            "cached": step_cached(step),
            "error": error_payload(step.error_json.as_deref()),
        })).collect::<Vec<_>>(),
        "cost": {
            "estimate": cost.estimate,
            "actual": cost.actual,
            "currency": cost.currency,
        },
    })
}

fn step_cached(step: &RunStepRecord) -> bool {
    step.metadata_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| value.get("cached").and_then(Value::as_bool))
        .unwrap_or(false)
}

fn error_payload(error_json: Option<&str>) -> Value {
    let Some(raw) = error_json.map(str::trim).filter(|value| !value.is_empty()) else {
        return Value::Null;
    };
    let summary = serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|value| error_summary_from_json(&value))
        .unwrap_or_else(|| truncate_chars(raw, 1200));
    json!({
        "summary": truncate_chars(first_line(summary.trim()), 160),
        "raw": truncate_chars(raw, 1200),
    })
}

fn error_summary_from_json(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    for key in ["error", "message", "reason"] {
        if let Some(summary) = object.get(key).and_then(Value::as_str) {
            let trimmed = summary.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or(value)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn history_payload(
    versions: &[VersionRecord],
    latest_run: Option<&RunRecord>,
    proposals: &[ProposalRecord],
) -> Vec<Value> {
    let mut history = versions
        .iter()
        .map(|version| {
            json!({
                "id": version.id,
                "kind": "version",
                "label": version.label,
                "time": version.created_at,
                "summary": format!("{} graph", version.source),
                "source": version.source,
            })
        })
        .collect::<Vec<_>>();

    if let Some(run) = latest_run {
        history.push(json!({
            "id": run.id,
            "kind": "run",
            "label": run.label,
            "time": run.created_at,
            "summary": run.status,
        }));
    }

    history.extend(proposals.iter().map(|proposal| {
        json!({
            "id": proposal.id,
            "kind": "proposal",
            "label": proposal.title,
            "time": proposal.created_at,
            "summary": proposal.state,
        })
    }));

    history
}

fn node_category(node_type: &str) -> String {
    match node_type.split('.').next().unwrap_or("node") {
        "input" => "Input",
        "output" => "Output",
        "llm" => "Text",
        "image" => "Image",
        "video" => "Video",
        _ => "Node",
    }
    .to_owned()
}

fn node_summary(node_type: &str, params: &Value) -> String {
    params
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or(node_type)
        .to_owned()
}

#[derive(Debug, Clone)]
struct CostSummary {
    estimate: f64,
    actual: f64,
    currency: String,
}

fn summarize_cost(costs: &[CostLedgerRecord]) -> CostSummary {
    let mut summary = CostSummary {
        estimate: 0.0,
        actual: 0.0,
        currency: "USD".to_owned(),
    };
    for cost in costs {
        if summary.currency == "USD" {
            summary.currency = cost.currency.clone();
        }
        if cost.estimated {
            summary.estimate += cost.amount;
        } else {
            summary.actual += cost.amount;
        }
    }
    summary
}
