use std::path::Path;

use helixflow_graph::WorkflowGraph;
use helixflow_registry::NodeRegistry;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::OutputContract;
use crate::canvas_edit::{CanvasEditPlan, catalog_entries, compile_canvas_edit, inspect_canvas};
use crate::canvas_ops::{CanvasOpsContext, MAX_CANVAS_STATE_BYTES};

pub(crate) fn helixflow_dynamic_tools(
    root_dir: &Path,
    output_contract: Option<OutputContract>,
) -> Vec<Value> {
    if output_contract == Some(OutputContract::RouteJson) {
        return vec![json!({
            "type": "namespace",
            "name": "agent",
            "description": "Select the user-facing turn mode for this request.",
            "tools": [{
                "type": "function",
                "name": "select_turn_mode",
                "description": "Record the turn mode for this user request. It cannot modify the canvas or call providers.",
                "inputSchema": {
                    "type": "object",
                    "required": ["mode", "requestedAction"],
                    "properties": {
                        "mode": {
                            "oneOf": [
                                {
                                    "const": "chat",
                                    "description": "Answer a read-only question about the workspace or product."
                                },
                                {
                                    "const": "create_workflow",
                                    "description": "Create a new catalog-valid workflow on the canvas."
                                },
                                {
                                    "const": "modify_workflow",
                                    "description": "Change the structure or parameters of the existing workflow."
                                },
                                {
                                    "const": "debug_workflow",
                                    "description": "Diagnose a failed run or propose a minimal repair."
                                },
                                {
                                    "const": "run_request",
                                    "description": "Execute the existing workflow because the current user explicitly requests execution or new results."
                                }
                            ]
                        },
                        "requestedAction": {
                            "type": "string",
                            "description": "One concise sentence containing only the user-visible outcome requested by the current user; exclude routing, tools, files, and prior-turn actions.",
                            "minLength": 1,
                            "maxLength": 1024
                        }
                    },
                    "additionalProperties": false
                }
            }]
        })];
    }
    if !root_dir.join("ctx/canvas_state.json").is_file() {
        return Vec::new();
    }
    let mut tools = vec![inspect_tool()];
    match output_contract {
        Some(OutputContract::CanvasEditJson) => {
            tools.insert(0, catalog_tool());
            tools.push(edit_tool());
        }
        Some(OutputContract::RunRequestJson) => {
            tools.push(run_tool());
            tools.push(wait_tool());
        }
        Some(OutputContract::ReplyJson) => {
            tools.push(submit_reply_tool());
        }
        _ => {}
    }
    vec![json!({
        "type": "namespace",
        "name": "canvas",
        "description": "Operate the live Helixflow canvas. Catalog and inspect are free; edit writes the board; run spends credits through the backend.",
        "tools": tools
    })]
}

fn catalog_tool() -> Value {
    json!({
        "type": "function",
        "name": "catalog",
        "description": "List node types available on this canvas. Call with no types for the index, then with types to load full configs.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "types": {
                    "type": "array",
                    "items": { "type": "string" },
                    "maxItems": 32
                }
            },
            "additionalProperties": false
        }
    })
}

fn inspect_tool() -> Value {
    json!({
        "type": "function",
        "name": "inspect",
        "description": "Read the current canvas. Filter by action, query, type, or ids instead of dumping the whole board.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["nodes", "node", "edges"]
                },
                "query": { "type": "string" },
                "node_id": { "type": "string" },
                "node_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "maxItems": 64
                },
                "node_types": {
                    "type": "array",
                    "items": { "type": "string" },
                    "maxItems": 32
                },
                "limit": { "type": "integer", "minimum": 1, "maximum": 200 }
            },
            "additionalProperties": false
        }
    })
}

fn edit_tool() -> Value {
    json!({
        "type": "function",
        "name": "edit",
        "description": "Apply one coherent set of canvas operations. This writes the live board; it does not call providers.",
        "inputSchema": {
            "type": "object",
            "required": ["operations"],
            "properties": {
                "operations": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 64,
                    "items": { "type": "object" }
                },
                "summary": { "type": "string", "maxLength": 4096 }
            },
            "additionalProperties": false
        }
    })
}

fn run_tool() -> Value {
    json!({
        "type": "function",
        "name": "run",
        "description": "Request execution of named canvas nodes. Upstream is resolved from the graph. Backend estimates cost; this never confirms or dispatches a provider.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "node_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "maxItems": 64
                },
                "summary": { "type": "string", "minLength": 1, "maxLength": 4096 }
            },
            "additionalProperties": false
        }
    })
}

fn wait_tool() -> Value {
    json!({
        "type": "function",
        "name": "wait",
        "description": "Check a started run only when the user asked to wait or this turn cannot continue without the result.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "run_id": { "type": "string" }
            },
            "additionalProperties": false
        }
    })
}

fn submit_reply_tool() -> Value {
    json!({
        "type": "function",
        "name": "submit_reply",
        "description": "Submit a read-only chat answer for backend validation. This cannot modify or run the canvas.",
        "inputSchema": {
            "type": "object",
            "required": ["message"],
            "properties": {
                "message": { "type": "string", "minLength": 1, "maxLength": 65536 }
            },
            "additionalProperties": false
        }
    })
}

pub(crate) async fn dispatch_dynamic_tool(
    matches_turn: bool,
    namespace: Option<&str>,
    tool: Option<&str>,
    output_contract: Option<OutputContract>,
    root_dir: &Path,
    out_dir: &Path,
    arguments: &Value,
) -> Value {
    match (matches_turn, namespace, tool) {
        (true, Some("canvas"), Some("catalog"))
            if output_contract == Some(OutputContract::CanvasEditJson) =>
        {
            canvas_catalog_tool_result(root_dir, arguments).await
        }
        (true, Some("canvas"), Some("inspect")) => {
            canvas_inspect_tool_result(root_dir, arguments).await
        }
        (true, Some("canvas"), Some("edit"))
            if output_contract == Some(OutputContract::CanvasEditJson) =>
        {
            submit_edit_tool_result(root_dir, out_dir, arguments).await
        }
        (true, Some("canvas"), Some("run"))
            if output_contract == Some(OutputContract::RunRequestJson) =>
        {
            request_run_tool_result(out_dir, arguments).await
        }
        (true, Some("canvas"), Some("wait"))
            if output_contract == Some(OutputContract::RunRequestJson) =>
        {
            canvas_wait_tool_result(root_dir, arguments).await
        }
        (true, Some("canvas"), Some("submit_reply"))
            if output_contract == Some(OutputContract::ReplyJson) =>
        {
            submit_reply_tool_result(out_dir, arguments).await
        }
        (true, Some("agent"), Some("select_turn_mode"))
            if output_contract == Some(OutputContract::RouteJson) =>
        {
            submit_route_tool_result(out_dir, arguments).await
        }
        _ => failed_tool_result("Unsupported or stale Helixflow dynamic tool call."),
    }
}

async fn canvas_catalog_tool_result(root_dir: &Path, arguments: &Value) -> Value {
    let path = root_dir.join("ctx/node_defs/catalog.json");
    match read_json_file(&path).await {
        Ok(catalog) => {
            let types = arguments
                .get("types")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            json_tool_result(&catalog_entries(&catalog, &types))
        }
        Err(error) => failed_tool_result(&format!("Canvas catalog is unavailable: {error}")),
    }
}

pub(crate) async fn canvas_inspect_tool_result(root_dir: &Path, arguments: &Value) -> Value {
    let path = root_dir.join("ctx/canvas_state.json");
    match read_json_file(&path).await {
        Ok(state) => json_tool_result(&inspect_canvas(&state, arguments)),
        Err(error) => failed_tool_result(&format!("Canvas state is unavailable: {error}")),
    }
}

pub(crate) async fn submit_edit_tool_result(
    root_dir: &Path,
    out_dir: &Path,
    arguments: &Value,
) -> Value {
    let plan: CanvasEditPlan = match serde_json::from_value(arguments.clone()) {
        Ok(plan) => plan,
        Err(error) => {
            return failed_tool_result(&format!("canvas.edit arguments are invalid: {error}"));
        }
    };
    let captured = capture_json_output(
        out_dir,
        arguments,
        "canvas_edit.json",
        "Canvas edit",
        "Canvas edit captured. The backend will version it; providers were not called.",
    )
    .await;
    if captured["success"] != true {
        return captured;
    }
    let graph_path = root_dir.join("ctx/graph.json");
    let graph: WorkflowGraph = match std::fs::read(&graph_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    {
        Some(graph) => graph,
        None => {
            return json!({
                "contentItems": [{
                    "type": "inputText",
                    "text": "Canvas edit captured. Live preview is unavailable because ctx/graph.json could not be read."
                }],
                "success": true
            });
        }
    };
    match compile_canvas_edit(&plan, &graph, &NodeRegistry::builtin()) {
        Ok(compiled) => {
            if let Err(error) = rewrite_live_canvas_state(root_dir, &compiled.preview) {
                return json!({
                    "contentItems": [{
                        "type": "inputText",
                        "text": format!(
                            "Canvas edit captured, but the live board could not be refreshed: {error}"
                        )
                    }],
                    "success": true
                });
            }
            json!({
                "contentItems": [{
                    "type": "inputText",
                    "text": format!(
                        "Canvas updated in place ({} operations). Inspect to read the live board.",
                        plan.operations.len()
                    )
                }],
                "success": true
            })
        }
        Err(error) => json!({
            "contentItems": [{
                "type": "inputText",
                "text": format!(
                    "Canvas edit captured with code {}. Backend will clarify or reject it: {error}",
                    error.code()
                )
            }],
            "success": true
        }),
    }
}

pub(crate) async fn request_run_tool_result(out_dir: &Path, arguments: &Value) -> Value {
    let node_ids = arguments
        .get("node_ids")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let summary = arguments
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Run selected canvas nodes.")
        .to_owned();
    let captured = json!({
        "action": "request_confirmation",
        "summary": summary,
        "node_ids": node_ids,
    });
    capture_json_output(
        out_dir,
        &captured,
        "run_request.json",
        "Run request",
        "Run request captured for backend estimation and cost gating; no provider was dispatched.",
    )
    .await
}

async fn canvas_wait_tool_result(root_dir: &Path, arguments: &Value) -> Value {
    let requested = arguments.get("run_id").and_then(Value::as_str);
    let path = root_dir.join("ctx/run_status.json");
    if let Ok(status) = read_json_file(&path).await {
        let current_id = status.get("run_id").and_then(Value::as_str);
        if requested.is_none() || requested == current_id {
            return json_tool_result(&status);
        }
        return json_tool_result(&json!({
            "status": "unknown",
            "run_id": requested,
            "message": "That run is not in the current canvas context."
        }));
    }
    json_tool_result(&json!({
        "status": "pending_backend",
        "run_id": requested,
        "message": "No run is in context yet. canvas.run only requests execution; progress appears on the canvas after the backend cost gate."
    }))
}

pub(crate) async fn submit_reply_tool_result(out_dir: &Path, arguments: &Value) -> Value {
    capture_json_output(
        out_dir,
        arguments,
        "reply.json",
        "Chat reply",
        "Chat reply captured for backend validation; the canvas was not changed.",
    )
    .await
}

pub(crate) async fn submit_route_tool_result(out_dir: &Path, arguments: &Value) -> Value {
    capture_json_output(
        out_dir,
        arguments,
        "route.json",
        "Turn route",
        "Turn mode captured for backend validation; no user action has run.",
    )
    .await
}

async fn capture_json_output(
    out_dir: &Path,
    arguments: &Value,
    file_name: &str,
    label: &str,
    success_message: &str,
) -> Value {
    const MAX_OUTPUT_BYTES: usize = 256 * 1024;
    let bytes = match serde_json::to_vec(arguments) {
        Ok(bytes) if arguments.is_object() && bytes.len() <= MAX_OUTPUT_BYTES => bytes,
        Ok(_) => {
            return failed_tool_result(&format!(
                "{label} arguments must be an object no larger than 256 KiB."
            ));
        }
        Err(error) => {
            return failed_tool_result(&format!("{label} arguments are invalid: {error}"));
        }
    };
    let path = out_dir.join(file_name);
    let write = async {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .await?;
        file.write_all(&bytes).await?;
        file.flush().await
    }
    .await;
    match write {
        Ok(()) => json!({
            "contentItems": [{
                "type": "inputText",
                "text": success_message
            }],
            "success": true
        }),
        Err(error) => failed_tool_result(&format!("{label} could not be captured: {error}")),
    }
}

fn rewrite_live_canvas_state(root_dir: &Path, preview: &WorkflowGraph) -> Result<(), String> {
    let path = root_dir.join("ctx/canvas_state.json");
    let existing = std::fs::read(&path).map_err(|error| error.to_string())?;
    let existing: CanvasOpsContext =
        serde_json::from_slice(&existing).map_err(|error| error.to_string())?;
    let updated = CanvasOpsContext::from_graph(
        &existing.workspace_id,
        &existing.base_version_id,
        preview,
        existing.selection,
        existing.gates,
    )
    .with_preferred_model_id(existing.preferred_model_id);
    let bytes = serde_json::to_vec_pretty(&updated).map_err(|error| error.to_string())?;
    std::fs::write(path, bytes).map_err(|error| error.to_string())
}

async fn read_json_file(path: &Path) -> Result<Value, String> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| error.to_string())?;
    if metadata.len() > MAX_CANVAS_STATE_BYTES as u64 {
        return Err("canvas payload exceeds 256 KiB".to_owned());
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

fn json_tool_result(value: &Value) -> Value {
    match serde_json::to_string(value) {
        Ok(text) => json!({
            "contentItems": [{ "type": "inputText", "text": text }],
            "success": true
        }),
        Err(error) => failed_tool_result(&format!("tool result is invalid: {error}")),
    }
}

fn failed_tool_result(message: &str) -> Value {
    json!({
        "contentItems": [{ "type": "inputText", "text": message }],
        "success": false
    })
}
