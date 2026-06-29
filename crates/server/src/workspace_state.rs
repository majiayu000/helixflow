use axum::{
    Json,
    extract::{Path as AxumPath, State},
};
use helixflow_agent::TurnMode;
use helixflow_graph::{ProposalOp, WorkflowGraph};
use helixflow_store::{
    ArtifactRecord, CostLedgerRecord, MessageRecord, ProposalRecord, RunRecord, RunStepRecord,
    VersionRecord, WorkspaceRecord,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::graph_files::{blank_graph, read_graph_file, read_json_file};
use crate::workbench_payload::{
    ProposalPayload, output_payload_from_artifact, pending_confirmation_from_run,
    pending_proposal_payload_from_record,
};

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
        Some(version) => read_graph_file(&state.data_dir, &version.graph_path).await?,
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
    let (steps, artifacts, costs, event_seq) = match &latest_run {
        Some(run) => {
            let steps = state
                .store
                .run_steps(&run.id)
                .await
                .map_err(ApiError::store)?;
            let artifacts = state
                .store
                .run_artifacts(&run.id)
                .await
                .map_err(ApiError::store)?;
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
    ))
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
) -> Value {
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
        "chat": {
            "messages": messages.iter().map(chat_message_payload).collect::<Vec<_>>(),
        },
        "graph": graph_payload(graph, &step_by_node),
        "run": latest_run.map(|run| run_payload(run, steps, &run_cost)),
        "outputs": artifacts.iter().map(output_payload_from_artifact).collect::<Vec<_>>(),
        "history": history_payload(versions, latest_run, proposals),
        "pendingConfirmation": latest_run.and_then(|run| pending_confirmation_from_run(run, costs)),
        "pendingProposal": pending_proposal,
        "workflowGraph": graph,
    })
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

fn graph_payload(graph: &WorkflowGraph, step_by_node: &BTreeMap<&str, &RunStepRecord>) -> Value {
    json!({
        "nodes": graph.nodes.iter().map(|(id, node)| {
            let step = step_by_node.get(id.as_str()).copied();
            json!({
                "id": id,
                "nodeType": node.node_type,
                "title": node.title,
                "category": node_category(&node.node_type),
                "status": step.map(|item| item.state.as_str()).unwrap_or("queued"),
                "position": { "x": node.pos[0], "y": node.pos[1] },
                "provider": step.and_then(|item| item.provider.clone()),
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
            "error": error_payload(step.error_json.as_deref()),
        })).collect::<Vec<_>>(),
        "cost": {
            "estimate": cost.estimate,
            "actual": cost.actual,
            "currency": cost.currency,
        },
    })
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::extract::{Path as AxumPath, State};
    use helixflow_agent::{
        AgentError, AgentSessionRequest, ValidatedAgentProposal, ValidatedAgentReply,
    };
    use helixflow_graph::{GraphEdge, GraphNode};
    use helixflow_run::EventBus;
    use helixflow_store::{NewMessage, NewRun, NewRunStep, NewVersion, Store, VersionSource};
    use serde_json::json;
    use std::path::Path;
    use std::path::PathBuf;

    use super::*;
    use crate::app_state::{AppState, WorkbenchAgent};

    #[tokio::test]
    async fn workspace_state_uses_store_records_and_graph_file() {
        let (state, workspace_id, _dir) = state_with_workspace().await;
        state
            .store
            .create_message(NewMessage {
                workspace_id: &workspace_id,
                role: "user",
                kind: "text",
                text: Some("Build this"),
                ref_id: None,
                attachment_ids_json: Some(r#"{"turnMode":"modify_workflow"}"#),
            })
            .await
            .expect("create message");

        let body = workspace_state(AxumPath(workspace_id.clone()), State(state))
            .await
            .expect("workspace state")
            .0;

        assert_eq!(body["workspace"]["id"], workspace_id);
        assert_eq!(body["workspace"]["name"], "Store workspace");
        assert_eq!(body["chat"]["messages"][0]["text"], "Build this");
        assert_eq!(body["chat"]["messages"][0]["turnMode"], "modify_workflow");
        assert_eq!(body["graph"]["nodes"].as_array().expect("nodes").len(), 1);
        assert_eq!(body["run"], Value::Null);
        assert!(body["pendingConfirmation"].is_null());
        assert_ne!(body["workspace"]["name"], "Helixflow Demo");
    }

    #[tokio::test]
    async fn workspace_state_returns_blank_graph_without_current_version() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = open_store(dir.path()).await;
        let workspace = store
            .create_workspace("Empty workspace")
            .await
            .expect("create workspace");
        let state = test_state(store, dir.path().to_path_buf());

        let body = workspace_state(AxumPath(workspace.id), State(state))
            .await
            .expect("workspace state")
            .0;

        assert_eq!(body["graph"]["nodes"].as_array().expect("nodes").len(), 0);
        assert_eq!(
            body["workflowGraph"]["nodes"]
                .as_object()
                .expect("nodes")
                .len(),
            0
        );
        assert!(body["run"].is_null());
    }

    #[tokio::test]
    async fn workspace_state_includes_failed_run_error_payload() {
        let (state, workspace_id, _dir) = state_with_workspace().await;
        let version_id = state
            .store
            .workspace(&workspace_id)
            .await
            .expect("workspace")
            .cur_version_id
            .expect("current version");
        let run = state
            .store
            .create_run(NewRun {
                workspace_id: &workspace_id,
                version_id: &version_id,
                group_id: None,
                label: "Failed render",
                trigger: "manual",
                plan_json: None,
                estimate_json: None,
                status: "running",
            })
            .await
            .expect("create run");
        let step = state
            .store
            .create_run_step(NewRunStep {
                run_id: &run.id,
                node_id: "input",
                node_type: "input.text",
                provider: Some("mock"),
                state: "running",
            })
            .await
            .expect("create step");
        let error_json =
            r#"{"error":"provider rejected duration","trace":"stack line 1\nstack line 2"}"#;
        state
            .store
            .update_run_step_state(&step.id, "failed", Some(1.0), None, Some(error_json))
            .await
            .expect("fail step");
        state
            .store
            .update_run_status(&run.id, "failed", Some(error_json))
            .await
            .expect("fail run");

        let body = workspace_state(AxumPath(workspace_id), State(state))
            .await
            .expect("workspace state")
            .0;

        assert_eq!(body["run"]["status"], "failed");
        assert_eq!(
            body["run"]["error"]["summary"],
            "provider rejected duration"
        );
        assert!(
            body["run"]["error"]["raw"]
                .as_str()
                .expect("raw")
                .contains("stack line")
        );
        assert_eq!(
            body["run"]["steps"][0]["error"]["summary"],
            "provider rejected duration"
        );
        assert_eq!(body["graph"]["nodes"][0]["status"], "failed");
    }

    #[tokio::test]
    async fn workspace_state_rejects_missing_current_graph_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = open_store(dir.path()).await;
        let workspace = store
            .create_workspace("Broken workspace")
            .await
            .expect("create workspace");
        store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Missing graph",
                source: VersionSource::Manual,
                graph_path: "graphs/missing.json",
                graph_hash: "sha256:missing",
                parent_id: None,
            })
            .await
            .expect("create version");
        let state = test_state(store, dir.path().to_path_buf());

        let err = workspace_state(AxumPath(workspace.id), State(state))
            .await
            .expect_err("missing graph file should error");

        assert_eq!(err.status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        assert!(err.message.contains("graphs/missing.json"));
    }

    async fn state_with_workspace() -> (AppState, String, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_dir = dir.path().to_path_buf();
        let store = open_store(&data_dir).await;
        let workspace = store
            .create_workspace("Store workspace")
            .await
            .expect("create workspace");
        tokio::fs::create_dir_all(data_dir.join("graphs"))
            .await
            .expect("create graph dir");
        let graph_path = "graphs/current.json";
        tokio::fs::write(
            data_dir.join(graph_path),
            serde_json::to_vec(&sample_graph()).expect("graph json"),
        )
        .await
        .expect("write graph");
        store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Current graph",
                source: VersionSource::Manual,
                graph_path,
                graph_hash: "sha256:current",
                parent_id: None,
            })
            .await
            .expect("create version");

        let state = test_state(store, data_dir);
        (state, workspace.id, dir)
    }

    async fn open_store(data_dir: &Path) -> Store {
        let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
        Store::open(&database_url).await.expect("open store")
    }

    fn test_state(store: Store, data_dir: PathBuf) -> AppState {
        AppState::with_store_agent(
            EventBus::new(16),
            store,
            data_dir.clone(),
            Arc::new(NoopWorkbenchAgent),
            data_dir.join("sessions"),
        )
    }

    fn sample_graph() -> WorkflowGraph {
        WorkflowGraph {
            schema_version: 1,
            nodes: BTreeMap::from([(
                "input".to_owned(),
                GraphNode {
                    node_type: "input.text".to_owned(),
                    title: "Input".to_owned(),
                    params: json!({ "summary": "Source text" }),
                    pos: [10.0, 20.0],
                },
            )]),
            edges: vec![GraphEdge {
                from: ["input".to_owned(), "text".to_owned()],
                to: ["input".to_owned(), "text".to_owned()],
                edge_type: "text".to_owned(),
            }],
        }
    }

    struct NoopWorkbenchAgent;

    #[async_trait]
    impl WorkbenchAgent for NoopWorkbenchAgent {
        async fn answer_chat(
            &self,
            _request: AgentSessionRequest,
        ) -> Result<ValidatedAgentReply, AgentError> {
            Err(AgentError::Runtime("noop agent".to_owned()))
        }

        async fn propose_graph_change(
            &self,
            _request: AgentSessionRequest,
        ) -> Result<ValidatedAgentProposal, AgentError> {
            Err(AgentError::Runtime("noop agent".to_owned()))
        }
    }
}
