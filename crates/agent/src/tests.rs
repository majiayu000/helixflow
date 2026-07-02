use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use helixflow_gateway::RuntimeProvider;
use helixflow_graph::{GraphEdge, GraphNode, ProposalKind};
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
                },
            ),
        ]),
        edges: vec![GraphEdge {
            from: ["input".to_owned(), "text".to_owned()],
            to: ["video".to_owned(), "prompt".to_owned()],
            edge_type: "text".to_owned(),
        }],
    }
}

fn request(dir: &tempfile::TempDir) -> AgentSessionRequest {
    AgentSessionRequest {
        workspace_id: "ws_1".to_owned(),
        base_version_id: "ver_1".to_owned(),
        user_message: "make it shorter".to_owned(),
        graph: sample_graph(),
        provider_catalog: RuntimeProvider::mock().catalog_snapshot(),
        run_context: None,
        sessions_dir: dir.path().join("agent_sessions"),
        mode: TurnMode::ModifyWorkflow,
        skill: AgentSkill::ModifyWorkflow,
        canvas_context: None,
    }
}

fn chat_request(dir: &tempfile::TempDir) -> AgentSessionRequest {
    AgentSessionRequest {
        workspace_id: "ws_1".to_owned(),
        base_version_id: "ver_1".to_owned(),
        user_message: "你好，你是谁？".to_owned(),
        graph: sample_graph(),
        provider_catalog: RuntimeProvider::mock().catalog_snapshot(),
        run_context: None,
        sessions_dir: dir.path().join("agent_sessions"),
        mode: TurnMode::Chat,
        skill: AgentSkill::Chat,
        canvas_context: None,
    }
}

#[test]
fn reports_module_name() {
    assert_eq!(module_name(), "agent");
}

#[test]
fn creates_ctx_out_contract_without_provider_secret_values() {
    let dir = tempfile::tempdir().expect("temp dir");
    let session = create_session_contract(&request(&dir)).expect("session");

    assert!(session.ctx_dir.join("graph.json").exists());
    assert!(session.ctx_dir.join("node_defs/catalog.json").exists());
    assert!(
        session
            .ctx_dir
            .join("workflow_backends/catalog.json")
            .exists()
    );
    assert!(
        session
            .ctx_dir
            .join("runtime_providers/catalog.json")
            .exists()
    );
    assert!(session.ctx_dir.join("api_connectors/catalog.json").exists());
    assert!(session.ctx_dir.join("skills/modify_workflow.md").exists());
    assert!(session.out_dir.exists());

    let ctx = fs::read_to_string(session.ctx_dir.join("instructions.md")).expect("ctx");
    assert!(ctx.contains("Write exactly one result file: `out/proposal.json`"));
    assert!(session.ctx_dir.join("canvas_state.json").exists());
    assert!(session.ctx_dir.join("canvas_ops.json").exists());
    assert!(ctx.contains("Top-level keys must be exactly"));
    assert!(ctx.contains("Bounded canvas ops"));
    assert!(ctx.contains("propose_layout"));
    assert!(ctx.contains("ctx/workflow_backends/catalog.json"));
    assert!(ctx.contains("ctx/runtime_providers/catalog.json"));
    assert!(ctx.contains("ctx/api_connectors/catalog.json"));
    assert!(ctx.contains("\"base_version_id\""));
    assert!(ctx.contains("\"ops\""));
    assert!(ctx.contains("Do not put top-level \"schema_version\", \"nodes\", or \"edges\""));
    assert!(ctx.contains("\"op\":\"add_node\""));
    assert!(ctx.contains("\"node_type\":\"input.text\""));
    assert!(ctx.contains("\"to\":[\"output\",\"artifact\"],\"edge_type\":\"artifact\""));
    assert_no_raw_auth_material(&ctx);

    let workflow_catalog =
        fs::read_to_string(session.ctx_dir.join("workflow_backends/catalog.json"))
            .expect("workflow backend catalog");
    let runtime_catalog =
        fs::read_to_string(session.ctx_dir.join("runtime_providers/catalog.json"))
            .expect("runtime provider catalog");
    let api_catalog = fs::read_to_string(session.ctx_dir.join("api_connectors/catalog.json"))
        .expect("api connector catalog");
    assert!(runtime_catalog.contains("\"id\": \"mock\""));
    assert!(runtime_catalog.contains("\"kind\": \"local_test\""));
    assert!(api_catalog.contains("\"capability\": \"text_to_video\""));
    assert_no_raw_auth_material(&workflow_catalog);
    assert_no_raw_auth_material(&runtime_catalog);
    assert_no_raw_auth_material(&api_catalog);
}

fn assert_no_raw_auth_material(content: &str) {
    for needle in [
        "PROVIDER_API_KEY",
        "Authorization",
        "Bearer ",
        "signed_url",
        "signedUrl",
        "access_token",
        "refresh_token",
        "client_secret",
        "secret_key",
        "SECRET=",
        "file://",
        "/Users/",
    ] {
        assert!(!content.contains(needle), "found raw auth marker {needle}");
    }
}

#[test]
fn writes_compact_canvas_state_with_filtered_selection() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut input = request(&dir);
    input.canvas_context = Some(CanvasOpsContext::from_graph(
        &input.workspace_id,
        &input.base_version_id,
        &input.graph,
        CanvasSelection {
            node_ids: vec![
                "video".to_owned(),
                "missing".to_owned(),
                "video".to_owned(),
                "input".to_owned(),
            ],
        },
        CanvasGateState {
            pending_proposal: true,
            pending_confirmation: false,
        },
    ));
    let session = create_session_contract(&input).expect("session");

    let canvas: Value = serde_json::from_slice(
        &fs::read(session.ctx_dir.join("canvas_state.json")).expect("canvas state"),
    )
    .expect("canvas json");
    assert_eq!(canvas["workspace_id"], "ws_1");
    assert_eq!(canvas["base_version_id"], "ver_1");
    assert_eq!(canvas["graph"]["node_count"], 2);
    assert_eq!(canvas["selection"]["node_ids"], json!(["video", "input"]));
    assert_eq!(canvas["gates"]["pending_proposal"], true);
    assert!(canvas.to_string().contains("video.text_to_video"));
    assert!(!canvas.to_string().contains("OPENAI_API_KEY"));
}

#[test]
fn chat_contract_skips_graph_context_and_records_prompt_metadata() {
    let dir = tempfile::tempdir().expect("temp dir");
    let session = create_session_contract(&chat_request(&dir)).expect("session");

    assert_eq!(session.mode, TurnMode::Chat);
    assert_eq!(session.output_contract, OutputContract::ReplyJson);
    assert_eq!(session.prompt_metadata.mode, TurnMode::Chat);
    assert_eq!(
        session.prompt_metadata.output_contract,
        OutputContract::ReplyJson
    );
    assert!(!session.ctx_dir.join("graph.json").exists());
    assert!(!session.ctx_dir.join("node_defs/catalog.json").exists());
    assert!(
        !session
            .ctx_dir
            .join("runtime_providers/catalog.json")
            .exists()
    );
    assert!(!session.ctx_dir.join("canvas_state.json").exists());
    assert!(!session.ctx_dir.join("canvas_ops.json").exists());

    let ctx = fs::read_to_string(session.ctx_dir.join("instructions.md")).expect("ctx");
    assert!(ctx.contains("Mode: Chat"));
    assert!(ctx.contains("out/reply.json"));
    assert!(ctx.contains("Do not read `ctx/graph.json`"));
    assert!(ctx.contains("create, modify, run, or debug a workflow"));

    let metadata: Value = serde_json::from_slice(
        &fs::read(session.root_dir.join("prompt_metadata.json")).expect("metadata"),
    )
    .expect("metadata json");
    assert_eq!(metadata["mode"], "chat");
    assert_eq!(metadata["output_contract"], "reply_json");
    assert_eq!(
        metadata["sections"][0]["key"],
        Value::String("mode_override".to_owned())
    );
}

#[test]
fn classifies_run_requests_without_falling_back_to_workflow_change() {
    assert_eq!(
        classify_turn_mode("运行当前 workflow", &sample_graph())
            .expect("mode")
            .mode,
        TurnMode::RunRequest
    );
    assert_eq!(
        classify_turn_mode("你好", &sample_graph())
            .expect("mode")
            .mode,
        TurnMode::Chat
    );
    let fallback = classify_turn_mode("继续", &sample_graph()).expect("mode");
    assert_eq!(fallback.mode, TurnMode::Chat);
    assert_eq!(fallback.source, TurnModeSource::AmbiguousFallback);
    assert!(classify_turn_mode("   ", &sample_graph()).is_err());
}

#[test]
fn classifies_modify_requests_before_broad_workflow_creation() {
    assert_eq!(
        classify_turn_mode("modify workflow duration", &sample_graph())
            .expect("mode")
            .mode,
        TurnMode::ModifyWorkflow
    );
    assert_eq!(
        classify_turn_mode("把工作流时长改短", &sample_graph())
            .expect("mode")
            .mode,
        TurnMode::ModifyWorkflow
    );
    assert_eq!(
        classify_turn_mode("创建一个 workflow", &sample_graph())
            .expect("mode")
            .mode,
        TurnMode::CreateWorkflow
    );
    let fallback = classify_turn_mode("workflow", &sample_graph()).expect("mode");
    assert_eq!(fallback.mode, TurnMode::Chat);
    assert_eq!(fallback.source, TurnModeSource::AmbiguousFallback);
    let empty_graph = WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::new(),
        edges: Vec::new(),
    };
    assert_eq!(
        classify_turn_mode("workflow", &empty_graph)
            .expect("mode")
            .mode,
        TurnMode::CreateWorkflow
    );
}

#[test]
fn codex_runtime_uses_array_command_in_session_dir() {
    let dir = tempfile::tempdir().expect("temp dir");
    let session = create_session_contract(&request(&dir)).expect("session");
    let command = CodexRuntime::new("codex").command_spec(&session);

    assert_eq!(command.program, PathBuf::from("codex"));
    assert_eq!(command.cwd, session.root_dir);
    assert!(command.env_clear);
    assert_eq!(
        command.env.get("HOME"),
        Some(&session.root_dir.display().to_string())
    );
    assert!(command.args.iter().any(|arg| arg == "--cd"));
    assert!(command.args.iter().any(|arg| arg == "--json"));
    assert!(
        command
            .args
            .windows(2)
            .any(|args| args == ["--sandbox", "workspace-write"])
    );
    assert!(!command.args.join(" ").contains("API_KEY"));
}

#[test]
fn safe_runtime_env_removes_provider_secret_keys() {
    let dir = tempfile::tempdir().expect("temp dir");
    let env = runtime::safe_runtime_env(
        [
            ("PATH".to_owned(), "/bin".to_owned()),
            ("CODEX_HOME".to_owned(), "/tmp/codex".to_owned()),
            ("OPENAI_API_KEY".to_owned(), "secret".to_owned()),
            ("PROVIDER_TOKEN".to_owned(), "secret".to_owned()),
            ("AWS_SECRET_ACCESS_KEY".to_owned(), "secret".to_owned()),
        ],
        dir.path(),
    );

    assert_eq!(env.get("PATH"), Some(&"/bin".to_owned()));
    assert_eq!(env.get("CODEX_HOME"), Some(&"/tmp/codex".to_owned()));
    assert_eq!(env.get("HOME"), Some(&dir.path().display().to_string()));
    assert!(!env.contains_key("OPENAI_API_KEY"));
    assert!(!env.contains_key("PROVIDER_TOKEN"));
    assert!(!env.contains_key("AWS_SECRET_ACCESS_KEY"));
}

#[test]
fn safe_runtime_env_derives_codex_home_without_exposing_real_home() {
    let dir = tempfile::tempdir().expect("temp dir");
    let env = runtime::safe_runtime_env(
        [("HOME".to_owned(), "/Users/example".to_owned())],
        dir.path(),
    );

    assert_eq!(
        env.get("CODEX_HOME"),
        Some(&"/Users/example/.codex".to_owned())
    );
    assert_eq!(env.get("HOME"), Some(&dir.path().display().to_string()));
}

#[test]
fn rejects_unknown_fields_in_proposal_output() {
    let dir = tempfile::tempdir().expect("temp dir");
    let session = create_session_contract(&request(&dir)).expect("session");
    write_proposal(
        &session,
        json!({
            "base_version_id": "ver_1",
            "kind": "modify",
            "title": "Bad proposal",
            "summary": "Contains untrusted field",
            "ops": [],
            "api_key": "should-not-be-here"
        }),
    );

    let err = read_validated_proposal(&session, &sample_graph(), "ver_1")
        .expect_err("unknown field should fail");

    assert!(err.to_string().contains("unknown field"));
}

#[test]
fn rejects_unknown_fields_nested_in_proposal_ops() {
    let dir = tempfile::tempdir().expect("temp dir");
    let session = create_session_contract(&request(&dir)).expect("session");
    write_proposal(
        &session,
        json!({
            "base_version_id": "ver_1",
            "kind": "modify",
            "title": "Bad nested proposal",
            "summary": "Contains ignored nested data",
            "ops": [{
                "op": "set_param",
                "id": "video",
                "key": "duration_sec",
                "value": 3,
                "provider_secret": "should-not-be-ignored"
            }]
        }),
    );

    let err = read_validated_proposal(&session, &sample_graph(), "ver_1")
        .expect_err("nested unknown field should fail");

    assert!(err.to_string().contains("unknown field"));
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_proposal_output() {
    let dir = tempfile::tempdir().expect("temp dir");
    let session = create_session_contract(&request(&dir)).expect("session");
    let outside = dir.path().join("outside.json");
    fs::write(&outside, "{}").expect("outside");
    std::os::unix::fs::symlink(&outside, session.out_dir.join("proposal.json"))
        .expect("symlink proposal");

    let err = read_validated_proposal(&session, &sample_graph(), "ver_1")
        .expect_err("symlink output should fail");

    assert!(err.to_string().contains("invalid agent output file"));
}

#[test]
fn validates_proposal_output_before_returning() {
    let dir = tempfile::tempdir().expect("temp dir");
    let session = create_session_contract(&request(&dir)).expect("session");
    write_valid_proposal(&session);

    let proposal = read_validated_proposal(&session, &sample_graph(), "ver_1").expect("proposal");

    assert_eq!(proposal.session_id, session.id);
    assert_eq!(proposal.proposal.kind, ProposalKind::Modify);
    assert_eq!(
        proposal.proposal.preview_graph.nodes["video"].params["duration_sec"],
        3
    );
}

#[tokio::test]
async fn service_streams_agent_status_and_reads_runtime_proposal() {
    let dir = tempfile::tempdir().expect("temp dir");
    let events = EventBus::new(16);
    let service = AgentService::new(FakeRuntime::proposal(), events.clone());
    let mut receiver = events.subscribe();

    let proposal = service
        .propose_graph_change(request(&dir))
        .await
        .expect("proposal");

    assert_eq!(proposal.proposal.title, "Shorter video");
    assert!(
        proposal
            .agent_logs
            .iter()
            .any(|log| log.kind == "agent_log:status" && log.text == "drafting proposal")
    );
    assert!(proposal.agent_logs.iter().any(|log| {
        log.text.contains("Prompt telemetry: mode=modify_workflow")
            && log.text.contains("output_contract=proposal_json")
            && log.text.contains("mode_override")
    }));
    assert!(
        proposal.agent_logs.iter().any(|log| {
            log.kind == "agent_log:canvas_ops" && log.text.contains("read_selection")
        })
    );

    let mut event_names = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        event_names.push(event.ev);
    }
    assert!(event_names.iter().any(|event| event == "agent.status"));
    assert!(event_names.iter().any(|event| event == "agent.status.end"));
}

#[tokio::test]
async fn service_streams_agent_status_and_reads_chat_reply() {
    let dir = tempfile::tempdir().expect("temp dir");
    let events = EventBus::new(16);
    let service = AgentService::new(FakeRuntime::reply(), events.clone());
    let mut receiver = events.subscribe();

    let reply = service
        .answer_chat(chat_request(&dir))
        .await
        .expect("reply");

    assert_eq!(reply.message, "我是 Helixflow agent。");
    assert!(
        reply
            .agent_logs
            .iter()
            .any(|log| log.kind == "agent_log:status" && log.text == "drafting reply")
    );
    assert!(reply.agent_logs.iter().any(|log| {
        log.text.contains("Prompt telemetry: mode=chat")
            && log.text.contains("output_contract=reply_json")
            && log.text.contains("mode_override")
            && !log.text.contains("你好，你是谁")
    }));

    let mut event_names = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        event_names.push(event.ev);
    }
    assert!(event_names.iter().any(|event| event == "agent.status"));
    assert!(event_names.iter().any(|event| event == "agent.status.end"));
}

fn write_valid_proposal(session: &AgentSession) {
    write_proposal(
        session,
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
        }),
    );
}

fn write_proposal(session: &AgentSession, value: Value) {
    fs::write(
        session.out_dir.join("proposal.json"),
        serde_json::to_vec(&value).expect("proposal json"),
    )
    .expect("write proposal");
}

#[derive(Clone, Default)]
struct FakeRuntime {
    events: Arc<Mutex<VecDeque<RuntimeEvent>>>,
    output: FakeOutput,
}

impl FakeRuntime {
    fn proposal() -> Self {
        Self {
            events: Arc::new(Mutex::new(VecDeque::from([
                RuntimeEvent::Status {
                    message: "drafting proposal".to_owned(),
                },
                RuntimeEvent::Finished,
            ]))),
            output: FakeOutput::Proposal,
        }
    }

    fn reply() -> Self {
        Self {
            events: Arc::new(Mutex::new(VecDeque::from([
                RuntimeEvent::Status {
                    message: "drafting reply".to_owned(),
                },
                RuntimeEvent::Finished,
            ]))),
            output: FakeOutput::Reply,
        }
    }
}

#[derive(Clone, Copy, Default)]
enum FakeOutput {
    #[default]
    Proposal,
    Reply,
}

#[async_trait]
impl AgentRuntime for FakeRuntime {
    fn id(&self) -> &'static str {
        "fake"
    }

    async fn start(&self, session: AgentSession) -> RuntimeResult<RuntimeHandle> {
        Ok(RuntimeHandle::new(
            self.id(),
            session.id,
            session.root_dir,
            session.out_dir,
        ))
    }

    async fn send(&self, handle: &RuntimeHandle, _turn: AgentTurn) -> RuntimeResult<()> {
        match self.output {
            FakeOutput::Proposal => write_valid_proposal_to_path(&handle.out_dir),
            FakeOutput::Reply => write_reply_to_path(&handle.out_dir),
        }
    }

    async fn next_event(&self, _handle: &RuntimeHandle) -> Option<RuntimeEvent> {
        self.events.lock().expect("events").pop_front()
    }

    async fn cancel(&self, _handle: &RuntimeHandle) -> RuntimeResult<()> {
        Ok(())
    }
}

fn write_valid_proposal_to_path(out_dir: &Path) -> RuntimeResult<()> {
    fs::write(
        out_dir.join("proposal.json"),
        serde_json::to_vec(&json!({
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
        }))
        .map_err(|err| RuntimeError::Failed(err.to_string()))?,
    )
    .map_err(|err| RuntimeError::Failed(err.to_string()))
}

fn write_reply_to_path(out_dir: &Path) -> RuntimeResult<()> {
    fs::write(
        out_dir.join("reply.json"),
        serde_json::to_vec(&json!({ "message": "我是 Helixflow agent。" }))
            .map_err(|err| RuntimeError::Failed(err.to_string()))?,
    )
    .map_err(|err| RuntimeError::Failed(err.to_string()))
}
