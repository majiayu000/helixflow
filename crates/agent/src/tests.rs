use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use helixflow_graph::{GraphEdge, GraphNode};
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
                    node_type: "video.mock.text_to_video".to_owned(),
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
        sessions_dir: dir.path().join("agent_sessions"),
        skill: AgentSkill::ModifyWorkflow,
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
    assert!(session.ctx_dir.join("skills/modify_workflow.md").exists());
    assert!(session.out_dir.exists());

    let ctx = fs::read_to_string(session.ctx_dir.join("instructions.md")).expect("ctx");
    assert!(ctx.contains("Write exactly one result file under `out/`"));
    assert!(!ctx.contains("PROVIDER_API_KEY"));
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
    let env = safe_runtime_env(
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
    let env = safe_runtime_env(
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
    let service = AgentService::new(FakeRuntime::new(), events.clone());
    let mut receiver = events.subscribe();

    let proposal = service
        .propose_graph_change(request(&dir))
        .await
        .expect("proposal");

    assert_eq!(proposal.proposal.title, "Shorter video");

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
}

impl FakeRuntime {
    fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(VecDeque::from([
                RuntimeEvent::Status {
                    message: "drafting proposal".to_owned(),
                },
                RuntimeEvent::Finished,
            ]))),
        }
    }
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
        write_valid_proposal_to_path(&handle.out_dir)
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
