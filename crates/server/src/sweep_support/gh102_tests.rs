use helixflow_agent::{AgentSessionRequest, TurnMode};
use helixflow_gateway::RuntimeProvider;

use super::{handle_run_request_with_threshold_config, tests};

#[tokio::test]
async fn invalid_threshold_fails_before_creating_run() {
    let (state, workspace_id, version_id, _dir) = tests::state_with_workspace().await;

    let result = handle_run_request_with_threshold_config(
        &state,
        AgentSessionRequest {
            workspace_id: workspace_id.clone(),
            base_version_id: version_id,
            user_message: "运行当前 workflow".to_owned(),
            codex_thread_id: None,
            history: Vec::new(),
            graph: tests::seed_graph(),
            provider_catalog: RuntimeProvider::mock().catalog_snapshot(),
            run_context: None,
            sessions_dir: state.agent_sessions_dir.clone(),
            mode: TurnMode::RunRequest,
            skill: TurnMode::RunRequest.agent_skill(),
            canvas_context: None,
            use_intent_contract: false,
        },
        Some("not-a-number"),
    )
    .await;
    let Err(err) = result else {
        panic!("invalid threshold must fail");
    };

    assert!(
        err.message
            .contains("HELIXFLOW_AGENT_RUN_CONFIRMATION_THRESHOLD_USD")
    );
    assert!(
        state
            .store
            .latest_workspace_run(&workspace_id)
            .await
            .expect("latest run")
            .is_none()
    );
}
