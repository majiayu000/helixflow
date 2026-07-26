use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use helixflow_gateway::RuntimeProvider;
use helixflow_graph::{GraphEdge, GraphNode, WorkflowGraph};
use helixflow_run::EventBus;
use serde_json::{Value, json};

use super::*;

fn sample_graph() -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([
            (
                "input".to_owned(),
                GraphNode {
                    node_type: "input.text".to_owned(),
                    title: "Text".to_owned(),
                    params: json!({ "text": "make a product clip" }),
                    pos: [0.0, 0.0],
                    size: None,
                    semantics: None,
                },
            ),
            (
                "video".to_owned(),
                GraphNode {
                    node_type: "video.text_to_video".to_owned(),
                    title: "Video".to_owned(),
                    params: json!({
                        "prompt": "clean product shot",
                        "duration_sec": 5,
                        "aspect_ratio": "9:16"
                    }),
                    pos: [220.0, 0.0],
                    size: None,
                    semantics: None,
                },
            ),
        ]),
        edges: vec![GraphEdge {
            from: ["input".to_owned(), "text".to_owned()],
            to: ["video".to_owned(), "prompt".to_owned()],
            edge_type: "text".to_owned(),
        }],
        catalog_revision: None,
    }
}

fn modify_request(dir: &tempfile::TempDir) -> AgentSessionRequest {
    AgentSessionRequest {
        workspace_id: "ws_1".to_owned(),
        base_version_id: "ver_1".to_owned(),
        user_message: "make it shorter".to_owned(),
        history: Vec::new(),
        graph: sample_graph(),
        provider_catalog: RuntimeProvider::mock().catalog_snapshot(),
        run_context: None,
        sessions_dir: dir.path().join("agent_sessions"),
        mode: TurnMode::ModifyWorkflow,
        skill: AgentSkill::ModifyWorkflow,
        canvas_context: None,
        use_intent_contract: false,
    }
}

fn chat_request(dir: &tempfile::TempDir) -> AgentSessionRequest {
    AgentSessionRequest {
        workspace_id: "ws_1".to_owned(),
        base_version_id: "ver_1".to_owned(),
        user_message: "你好，你是谁？".to_owned(),
        history: Vec::new(),
        graph: sample_graph(),
        provider_catalog: RuntimeProvider::mock().catalog_snapshot(),
        run_context: None,
        sessions_dir: dir.path().join("agent_sessions"),
        mode: TurnMode::Chat,
        skill: AgentSkill::Chat,
        canvas_context: None,
        use_intent_contract: false,
    }
}

#[tokio::test]
async fn retry_loop_recovers_after_invalid_node_type() {
    let dir = tempfile::tempdir().expect("temp dir");
    let events = EventBus::new(64);
    let _receiver = events.subscribe();
    let runtime = ScriptedRuntime::new(vec![
        ScriptedRound::Proposal(invalid_node_type_proposal()),
        ScriptedRound::Proposal(valid_proposal()),
    ]);
    let service = AgentService::new(runtime.clone(), events.clone()).with_max_proposal_rounds(3);

    let proposal = service
        .propose_graph_change(modify_request(&dir))
        .await
        .expect("proposal");

    assert_eq!(proposal.proposal.title, "Shorter video");
    assert_eq!(runtime.sent_turns().len(), 2);
    assert_eq!(runtime.sent_turns()[0].message, "make it shorter");
    let retry_message = &runtime.sent_turns()[1].message;
    assert!(retry_message.contains("Previous proposal validation failed"));
    assert!(retry_message.contains("Retry round: 2/3"));
    assert!(retry_message.contains("Current graph context: workspace_id=ws_1"));
    assert!(retry_message.contains("unknown node type: unknown.node"));
    assert!(retry_message.contains("Last failed proposal summary: title=Bad node type"));
    assert!(!retry_message.contains("/Users/"));
    assert!(!retry_message.contains("/private/"));
    assert!(!retry_message.contains("file://"));
    assert!(!retry_message.contains("Bearer "));
    assert!(!retry_message.contains("SECRET="));
    assert!(proposal.agent_logs.iter().any(|log| {
        log.kind == "agent_log:error" && log.text.contains("unknown node type: unknown.node")
    }));
    assert!(proposal.agent_logs.iter().any(|log| {
        log.kind == "agent_log:status"
            && log
                .text
                .contains("retrying proposal generation (round 2/3)")
    }));
    assert!(
        proposal
            .agent_logs
            .iter()
            .any(|log| log.text == "proposal ready after round 2")
    );
}

#[tokio::test]
async fn retry_loop_exhausts_with_last_validation_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let events = EventBus::new(64);
    let mut receiver = events.subscribe();
    let runtime = ScriptedRuntime::new(vec![
        ScriptedRound::Proposal(invalid_node_type_proposal()),
        ScriptedRound::Proposal(invalid_node_type_proposal()),
    ]);
    let service = AgentService::new(runtime.clone(), events.clone()).with_max_proposal_rounds(2);

    let err = service
        .propose_graph_change(modify_request(&dir))
        .await
        .expect_err("retry exhaustion");

    assert!(
        err.to_string()
            .contains("proposal retry exhausted after 2 rounds")
    );
    assert!(err.to_string().contains("unknown node type: unknown.node"));
    assert_eq!(runtime.sent_turns().len(), 2);
    assert!(
        runtime.sent_turns()[1]
            .message
            .contains("unknown node type: unknown.node")
    );

    let mut statuses = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        if let Some(status) = event.data.get("status").and_then(Value::as_str) {
            statuses.push(status.to_owned());
        }
    }
    assert!(
        statuses
            .iter()
            .any(|status| status == "proposal.retry_exhausted")
    );
}

#[tokio::test]
async fn chat_turn_remains_single_round() {
    let dir = tempfile::tempdir().expect("temp dir");
    let events = EventBus::new(64);
    let _receiver = events.subscribe();
    let runtime = ScriptedRuntime::new(vec![ScriptedRound::Reply(json!({
        "message": "我是 Helixflow agent。"
    }))]);
    let service = AgentService::new(runtime.clone(), events.clone()).with_max_proposal_rounds(3);

    let reply = service
        .answer_chat(chat_request(&dir))
        .await
        .expect("reply");

    assert_eq!(reply.message, "我是 Helixflow agent。");
    assert_eq!(runtime.sent_turns().len(), 1);
    assert!(
        !runtime.sent_turns()[0]
            .message
            .contains("Previous proposal validation failed")
    );
}

#[derive(Clone)]
struct ScriptedRuntime {
    rounds: Arc<Mutex<VecDeque<ScriptedRound>>>,
    events: Arc<Mutex<VecDeque<RuntimeEvent>>>,
    sent_turns: Arc<Mutex<Vec<AgentTurn>>>,
}

impl ScriptedRuntime {
    fn new(rounds: Vec<ScriptedRound>) -> Self {
        Self {
            rounds: Arc::new(Mutex::new(VecDeque::from(rounds))),
            events: Arc::new(Mutex::new(VecDeque::new())),
            sent_turns: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn sent_turns(&self) -> Vec<AgentTurn> {
        self.sent_turns.lock().expect("sent turns").clone()
    }
}

#[derive(Clone)]
enum ScriptedRound {
    Proposal(Value),
    Reply(Value),
}

#[async_trait]
impl AgentRuntime for ScriptedRuntime {
    fn id(&self) -> &'static str {
        "scripted"
    }

    async fn start(&self, session: AgentSession) -> RuntimeResult<RuntimeHandle> {
        Ok(RuntimeHandle::new(
            self.id(),
            session.id,
            session.root_dir,
            session.out_dir,
        ))
    }

    async fn send(&self, handle: &RuntimeHandle, turn: AgentTurn) -> RuntimeResult<()> {
        self.sent_turns
            .lock()
            .expect("sent turns")
            .push(turn.clone());
        let round = self
            .rounds
            .lock()
            .expect("rounds")
            .pop_front()
            .ok_or_else(|| RuntimeError::Failed("missing scripted round".to_owned()))?;

        match round {
            ScriptedRound::Proposal(value) => {
                write_runtime_json(handle.out_dir.join("proposal.json"), value)?
            }
            ScriptedRound::Reply(value) => {
                write_runtime_json(handle.out_dir.join("reply.json"), value)?
            }
        }

        let round_number = self.sent_turns.lock().expect("sent turns").len();
        let mut events = self.events.lock().expect("events");
        events.push_back(RuntimeEvent::Status {
            message: format!("scripted runtime round {round_number}"),
        });
        events.push_back(RuntimeEvent::Finished);
        Ok(())
    }

    async fn next_event(&self, _handle: &RuntimeHandle) -> Option<RuntimeEvent> {
        self.events.lock().expect("events").pop_front()
    }

    async fn cancel(&self, _handle: &RuntimeHandle) -> RuntimeResult<()> {
        Ok(())
    }
}

fn write_runtime_json(path: impl AsRef<Path>, value: Value) -> RuntimeResult<()> {
    fs::write(
        path,
        serde_json::to_vec(&value).map_err(|err| RuntimeError::Failed(err.to_string()))?,
    )
    .map_err(|err| RuntimeError::Failed(err.to_string()))
}

fn invalid_node_type_proposal() -> Value {
    json!({
        "base_version_id": "ver_1",
        "kind": "modify",
        "title": "Bad node type",
        "summary": "Adds an unsupported node type.",
        "ops": [{
            "op": "add_node",
            "id": "broken",
            "node": {
                "node_type": "unknown.node",
                "title": "Unknown",
                "params": {},
                "pos": [480.0, 0.0]
            }
        }]
    })
}

fn valid_proposal() -> Value {
    json!({
        "base_version_id": "ver_1",
        "kind": "modify",
        "title": "Shorter video",
        "summary": "Set duration to three seconds.",
        "ops": [{
            "op": "set_param",
            "id": "video",
            "key": "duration_sec",
            "prev": 5,
            "value": 3
        }]
    })
}
