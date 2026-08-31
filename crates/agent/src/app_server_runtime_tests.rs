use std::fs;

use serde_json::{Value, json};

use super::{
    canvas_state_tool_result, completed_item_status, helixflow_dynamic_tools,
    notification_matches_turn, request_run_tool_result, required_string, started_item_status,
    submit_intent_tool_result, submit_proposal_tool_result, submit_reply_tool_result,
    submit_route_tool_result,
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
            "params": { "item": { "type": "dynamicToolCall", "tool": "canvas.get_state" } }
        }))
        .as_deref(),
        Some("Tool completed: canvas.get_state")
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

    let tools = helixflow_dynamic_tools(dir.path(), Some(crate::OutputContract::ProposalJson));
    assert_eq!(tools[0]["name"], "canvas");
    assert_eq!(tools[0]["tools"][0]["name"], "get_state");
    assert_eq!(tools[0]["tools"][1]["name"], "submit_proposal");
    let intent_tools = helixflow_dynamic_tools(dir.path(), Some(crate::OutputContract::IntentJson));
    assert_eq!(intent_tools[0]["tools"][1]["name"], "submit_intent");
    assert_eq!(
        intent_tools[0]["tools"][1]["inputSchema"]["properties"]["stages"]["items"]["properties"]["capabilityId"]
            ["enum"],
        json!(["prompt_writer", "text_to_image"])
    );
    let run_tools =
        helixflow_dynamic_tools(dir.path(), Some(crate::OutputContract::RunRequestJson));
    assert_eq!(run_tools[0]["tools"][1]["name"], "request_run");
    let reply_tools = helixflow_dynamic_tools(dir.path(), Some(crate::OutputContract::ReplyJson));
    assert_eq!(reply_tools[0]["tools"][1]["name"], "submit_reply");
    let result = canvas_state_tool_result(dir.path()).await;
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
async fn captures_proposals_without_applying_them() {
    let dir = tempfile::tempdir().expect("temp dir");
    let out_dir = dir.path().join("out");
    fs::create_dir(&out_dir).expect("out dir");
    let proposal = json!({
        "base_version_id": "ver_1",
        "kind": "modify",
        "title": "Move node",
        "summary": "Move one node.",
        "ops": [{ "op": "move_node", "id": "video", "pos": [10, 20] }]
    });

    let result = submit_proposal_tool_result(&out_dir, &proposal).await;

    assert_eq!(result["success"], true);
    let captured: Value = serde_json::from_slice(
        &fs::read(out_dir.join("proposal.json")).expect("captured proposal"),
    )
    .expect("proposal json");
    assert_eq!(captured, proposal);
}

#[tokio::test]
async fn captures_intents_without_mutating_the_canvas() {
    let dir = tempfile::tempdir().expect("temp dir");
    let out_dir = dir.path().join("out");
    fs::create_dir(&out_dir).expect("out dir");
    let intent = json!({
        "intentVersion": "1",
        "topology": "linear",
        "stages": [{
            "stageId": "s1",
            "capabilityId": "text_to_image",
            "inputFrom": [],
            "params": { "prompt": "a paper fox" }
        }],
        "outputStageIds": ["s1"]
    });

    let result = submit_intent_tool_result(&out_dir, &intent).await;

    assert_eq!(result["success"], true);
    let captured: Value =
        serde_json::from_slice(&fs::read(out_dir.join("intent.json")).expect("captured intent"))
            .expect("intent json");
    assert_eq!(captured, intent);
}

#[tokio::test]
async fn captures_run_requests_without_dispatching_providers() {
    let dir = tempfile::tempdir().expect("temp dir");
    let out_dir = dir.path().join("out");
    fs::create_dir(&out_dir).expect("out dir");
    let request = json!({
        "action": "request_confirmation",
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
    assert_eq!(captured, request);
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
