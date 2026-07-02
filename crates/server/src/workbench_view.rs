use std::fs;
use std::path::{Component, Path, PathBuf};

use helixflow_graph::WorkflowGraph;
use helixflow_registry::NodeRegistry;
use helixflow_store::{
    ArtifactRecord, MessageRecord, ProposalRecord, RunRecord, RunStepRecord, VersionRecord,
};
use serde_json::{Value, json};

pub(crate) fn message_json(message: &MessageRecord) -> Value {
    json!({
        "id": message.id,
        "role": message.role,
        "text": message.text.clone().unwrap_or_default(),
        "kind": message.kind,
        "label": message.ref_id,
        "time": message.created_at
    })
}

pub(crate) fn graph_nodes_json(graph: &WorkflowGraph, steps: &[RunStepRecord]) -> Vec<Value> {
    let registry = NodeRegistry::builtin();
    graph
        .nodes
        .iter()
        .map(|(id, node)| {
            let definition = registry.definition(&node.node_type).ok();
            let step = steps.iter().find(|step| step.node_id == *id);
            json!({
                "id": id,
                "nodeType": node.node_type,
                "title": node.title,
                "category": definition.map(|item| item.category.as_str()).unwrap_or("custom"),
                "status": step.map(|item| item.state.as_str()).unwrap_or("queued"),
                "position": { "x": node.pos[0], "y": node.pos[1] },
                "provider": definition.and_then(|item| item.provider.clone()),
                "summary": node.params.to_string()
            })
        })
        .collect()
}

pub(crate) fn run_json(
    run: Option<&RunRecord>,
    steps: &[RunStepRecord],
    plan: &helixflow_graph::ExecutionPlan,
) -> Value {
    if let Some(run) = run {
        return json!({
            "id": run.id,
            "label": run.label,
            "status": run.status,
            "steps": steps.iter().map(step_json).collect::<Vec<_>>(),
            "cost": cost_json(run)
        });
    }

    json!({
        "id": "",
        "label": "No run yet",
        "status": "queued",
        "steps": plan.steps.iter().map(|step| {
            json!({
                "nodeId": step.node_id,
                "title": step.node_id,
                "state": "queued",
                "provider": step.provider
            })
        }).collect::<Vec<_>>(),
        "cost": { "estimate": 0, "actual": 0, "currency": "USD" }
    })
}

pub(crate) fn output_json(artifact: &ArtifactRecord, data_dir: &Path) -> Value {
    let meta_value = artifact
        .meta_json
        .as_deref()
        .and_then(|meta| serde_json::from_str::<Value>(meta).ok());
    let title = meta_value
        .as_ref()
        .and_then(|meta| meta.get("title"))
        .and_then(Value::as_str)
        .or(artifact.node_id.as_deref())
        .unwrap_or(&artifact.kind);
    let mut value = json!({
        "id": artifact.id,
        "kind": artifact.kind,
        "title": title,
        "storageUri": artifact.storage_uri,
        "selected": artifact.selected,
        "meta": artifact.meta_json.clone().unwrap_or_else(|| artifact.created_at.clone()),
        "mime": artifact.mime
    });
    if let Some(preview) = artifact_preview_json(artifact, data_dir) {
        value["preview"] = preview;
    }
    value
}

pub(crate) fn history_json(
    versions: &[VersionRecord],
    runs: &[RunRecord],
    proposals: &[ProposalRecord],
) -> Vec<Value> {
    let mut items = Vec::new();
    items.extend(
        versions
            .iter()
            .filter(|version| version.label != "Empty workflow")
            .map(|version| {
                json!({
                    "id": version.id,
                    "kind": "version",
                    "label": version.label,
                    "time": version.created_at,
                    "summary": format!("Version {}", version.idx)
                })
            }),
    );
    items.extend(runs.iter().map(|run| {
        json!({
            "id": run.id,
            "kind": "run",
            "label": run.label,
            "time": run.created_at,
            "summary": run.status
        })
    }));
    items.extend(proposals.iter().map(|proposal| {
        json!({
            "id": proposal.id,
            "kind": "proposal",
            "label": proposal.title,
            "time": proposal.created_at,
            "summary": proposal.state
        })
    }));
    items
}

pub(crate) fn pending_confirmation_json(run: Option<&RunRecord>) -> Value {
    let Some(run) = run else {
        return Value::Null;
    };
    if run.status != "waiting_confirmation" {
        return Value::Null;
    }
    let cost = cost_json(run);
    json!({
        "id": run.id,
        "title": "Run requires confirmation",
        "summary": format!("Run `{}` through Atlas/API providers and local builtins.", run.label),
        "cost": {
            "amount": cost.get("estimate").and_then(Value::as_f64).unwrap_or(0.0),
            "currency": cost.get("currency").and_then(Value::as_str).unwrap_or("USD")
        }
    })
}

fn step_json(step: &RunStepRecord) -> Value {
    json!({
        "nodeId": step.node_id,
        "title": step.node_id,
        "state": step.state,
        "provider": step.provider
    })
}

fn cost_json(run: &RunRecord) -> Value {
    if let Some(estimate_json) = &run.estimate_json
        && let Ok(value) = serde_json::from_str::<Value>(estimate_json)
    {
        return json!({
            "estimate": value.get("amount").and_then(Value::as_f64).unwrap_or(0.0),
            "actual": 0,
            "currency": value.get("currency").and_then(Value::as_str).unwrap_or("USD")
        });
    }
    json!({ "estimate": 0, "actual": 0, "currency": "USD" })
}

fn artifact_preview_json(artifact: &ArtifactRecord, data_dir: &Path) -> Option<Value> {
    let kind = artifact.kind.as_str();
    let mime = artifact.mime.as_deref().unwrap_or_default();
    let preview_kind = if kind == "html" || mime == "text/html" {
        "html"
    } else if matches!(kind, "text" | "markdown" | "json") || mime.starts_with("text/") {
        "text"
    } else {
        return None;
    };
    let path = workspace_uri_path(data_dir, &artifact.storage_uri)?;
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > 1_048_576 {
        return None;
    }
    let content = fs::read_to_string(path).ok()?;
    Some(json!({
        "kind": preview_kind,
        "content": content
    }))
}

fn workspace_uri_path(data_dir: &Path, storage_uri: &str) -> Option<PathBuf> {
    let relative = storage_uri.strip_prefix("workspace://")?;
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return None;
    }
    Some(data_dir.join(relative_path))
}
