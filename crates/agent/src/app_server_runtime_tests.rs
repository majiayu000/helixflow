use std::fs;

use serde_json::{Value, json};

use super::{
    canvas_inspect_tool_result, completed_item_status, helixflow_dynamic_tools,
    notification_matches_turn, request_run_tool_result, required_string, started_item_status,
    submit_edit_tool_result, submit_reply_tool_result, submit_route_tool_result,
};

#[test]
fn parses_thread_identity_and_completed_items() {
    let response = json!({ "result": { "thread": { "id": "thr_1" } } });
    assert_eq!(
        required_string(&response, "/result/thread/id").unwrap(),
        "thr_1"
    );
    assert_eq!(
        completed_item_status(&json!({
            "params": { "item": { "type": "dynamicToolCall", "tool": "canvas.inspect" } }
        }))
        .as_deref(),
        Some("Tool completed: canvas.inspect")
    );
    assert_eq!(
        started_item_status(&json!({
            "params": { "item": { "type": "reasoning" } }
        }))
        .as_deref(),
        Some("Planning workflow")
    );
    assert_eq!(
        completed_item_status(&json!({
            "params": {
                "item": {
                    "type": "commandExecution",
                    "command": "printenv SUPER_SECRET"
                }
            }
        }))
        .as_deref(),
        Some("Restricted command completed")
    );
    let active = json!({
        "params": { "threadId": "thr_1", "turnId": "turn_1" }
    });
    assert!(notification_matches_turn(&active, "thr_1", "turn_1"));
    assert!(!notification_matches_turn(&active, "thr_1", "turn_stale"));
}

#[tokio::test]
async fn exposes_bounded_canvas_state_as_a_dynamic_tool() {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::create_dir(dir.path().join("ctx")).expect("ctx dir");
    fs::write(
        dir.path().join("ctx/canvas_state.json"),
        r#"{ "workspace_id": "ws_1", "graph": { "node_count": 1 } }"#,
    )
    .expect("canvas state");
    fs::create_dir_all(dir.path().join("ctx/node_defs")).expect("node catalog dir");
    fs::write(
        dir.path().join("ctx/node_defs/catalog.json"),
        r#"{
            "schema_version": 1,
            "nodes": [
                {"type":"llm.prompt_writer","capability":"prompt_writer"},
                {"type":"image.generate","capability":"text_to_image"},
                {"type":"input.text","capability":null}
            ]
        }"#,
    )
    .expect("node catalog");

    let tools = helixflow_dynamic_tools(dir.path(), Some(crate::OutputContract::CanvasEditJson));
    assert_eq!(tools[0]["name"], "canvas");
    assert_eq!(tools[0]["tools"][0]["name"], "catalog");
    assert_eq!(tools[0]["tools"][1]["name"], "inspect");
    assert_eq!(tools[0]["tools"][2]["name"], "edit");
    let run_tools =
        helixflow_dynamic_tools(dir.path(), Some(crate::OutputContract::RunRequestJson));
    assert_eq!(run_tools[0]["tools"][1]["name"], "run");
    assert_eq!(run_tools[0]["tools"][2]["name"], "wait");
    let reply_tools = helixflow_dynamic_tools(dir.path(), Some(crate::OutputContract::ReplyJson));
    assert_eq!(reply_tools[0]["tools"][1]["name"], "submit_reply");
    let result = canvas_inspect_tool_result(dir.path(), &json!({})).await;
    assert_eq!(result["success"], true);
    assert!(
        result["contentItems"][0]["text"]
            .as_str()
            .expect("text")
            .contains("ws_1")
    );
}

#[tokio::test]
async fn exposes_and_captures_semantic_routing_without_canvas_access() {
    let dir = tempfile::tempdir().expect("temp dir");
    let out_dir = dir.path().join("out");
    fs::create_dir(&out_dir).expect("out dir");

    let tools = helixflow_dynamic_tools(dir.path(), Some(crate::OutputContract::RouteJson));

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "agent");
    assert_eq!(tools[0]["tools"][0]["name"], "select_turn_mode");
    assert_eq!(tools[0]["tools"].as_array().expect("tools").len(), 1);
    assert_eq!(
        tools[0]["tools"][0]["inputSchema"]["required"],
        json!(["mode", "requestedAction"])
    );
    assert_eq!(
        tools[0]["tools"][0]["inputSchema"]["properties"]["mode"]["oneOf"][0]["const"],
        "chat"
    );
    assert!(
        tools[0]["tools"][0]["inputSchema"]["properties"]["mode"]["oneOf"][0]["description"]
            .as_str()
            .expect("chat description")
            .contains("read-only")
    );

    let route = json!({
        "mode": "modify_workflow",
        "requestedAction": "Change the second stage."
    });
    let result = submit_route_tool_result(&out_dir, &route).await;

    assert_eq!(result["success"], true);
    let captured: Value =
        serde_json::from_slice(&fs::read(out_dir.join("route.json")).expect("captured route"))
            .expect("route json");
    assert_eq!(captured, route);
}

#[tokio::test]
async fn captures_canvas_edits_and_refreshes_live_state() {
    let dir = tempfile::tempdir().expect("temp dir");
    let out_dir = dir.path().join("out");
    fs::create_dir(&out_dir).expect("out dir");
    fs::create_dir(dir.path().join("ctx")).expect("ctx dir");
    fs::write(
        dir.path().join("ctx/graph.json"),
        r#"{"schema_version":1,"nodes":{},"edges":[]}"#,
    )
    .expect("graph");
    fs::write(
        dir.path().join("ctx/canvas_state.json"),
        r#"{"schema_version":1,"workspace_id":"ws_1","base_version_id":"ver_1","graph":{"node_count":0,"edge_count":0,"nodes":[],"edges":[]},"selection":{"node_ids":[]},"gates":{"pending_proposal":false,"pending_confirmation":false}}"#,
    )
    .expect("canvas state");
    let edit = json!({
        "operations": [{
            "op": "add_node",
            "id": "s1",
            "node_type": "image.generate",
            "params": { "prompt": "a paper fox" }
        }]
    });

    let result = submit_edit_tool_result(dir.path(), &out_dir, &edit).await;

    assert_eq!(result["success"], true);
    let captured: Value =
        serde_json::from_slice(&fs::read(out_dir.join("canvas_edit.json")).expect("captured edit"))
            .expect("edit json");
    assert_eq!(captured, edit);
    let live = fs::read_to_string(dir.path().join("ctx/canvas_state.json")).expect("live canvas");
    assert!(live.contains("\"id\": \"s1\"") || live.contains("\"id\":\"s1\""));
}

#[tokio::test]
async fn captures_run_requests_without_dispatching_providers() {
    let dir = tempfile::tempdir().expect("temp dir");
    let out_dir = dir.path().join("out");
    fs::create_dir(&out_dir).expect("out dir");
    let request = json!({
        "node_ids": ["s1"],
        "summary": "Run the current workflow."
    });

    let result = request_run_tool_result(&out_dir, &request).await;

    assert_eq!(result["success"], true);
    assert!(
        result["contentItems"][0]["text"]
            .as_str()
            .expect("tool result")
            .contains("no provider was dispatched")
    );
    let captured: Value = serde_json::from_slice(
        &fs::read(out_dir.join("run_request.json")).expect("captured run request"),
    )
    .expect("run request json");
    assert_eq!(
        captured,
        json!({
            "action": "request_confirmation",
            "summary": "Run the current workflow.",
            "node_ids": ["s1"]
        })
    );
}

#[tokio::test]
async fn captures_chat_replies_without_canvas_mutation() {
    let dir = tempfile::tempdir().expect("temp dir");
    let out_dir = dir.path().join("out");
    fs::create_dir(&out_dir).expect("out dir");
    let reply = json!({ "message": "The workflow turns text into an image." });

    let result = submit_reply_tool_result(&out_dir, &reply).await;

    assert_eq!(result["success"], true);
    let captured: Value =
        serde_json::from_slice(&fs::read(out_dir.join("reply.json")).expect("captured reply"))
            .expect("reply json");
    assert_eq!(captured, reply);
}
