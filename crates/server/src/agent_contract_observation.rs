use helixflow_agent::AgentError;
use helixflow_store::{
    AgentContractMode, AgentContractOutcome, CompleteAgentContractObservation, StoreError,
};

use crate::app_state::AppState;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AgentContractAttribution {
    pub(crate) release_id: Option<String>,
    pub(crate) build_revision: Option<String>,
}

impl AgentContractAttribution {
    pub(crate) fn from_env() -> Result<Self, String> {
        Ok(Self {
            release_id: read_optional_identity("HELIXFLOW_RELEASE_ID", 64)?,
            build_revision: read_optional_identity("HELIXFLOW_BUILD_REVISION", 128)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentContractTurn {
    pub(crate) observation_id: String,
    pub(crate) contract_mode: AgentContractMode,
}

impl AgentContractTurn {
    pub(crate) fn new(observation_id: String, contract_mode: AgentContractMode) -> Self {
        Self {
            observation_id,
            contract_mode,
        }
    }

    pub(crate) fn completion<'a>(
        &'a self,
        workspace_id: &'a str,
        outcome: AgentContractOutcome,
        reason_code: &'a str,
        session_id: Option<&'a str>,
    ) -> CompleteAgentContractObservation<'a> {
        CompleteAgentContractObservation {
            observation_id: &self.observation_id,
            workspace_id,
            contract_mode: self.contract_mode,
            outcome,
            reason_code,
            session_id,
        }
    }
}

pub(crate) async fn finalize_agent_contract_error(
    state: &AppState,
    workspace_id: &str,
    turn: &AgentContractTurn,
    reason_code: &str,
    session_id: Option<&str>,
) -> Result<(), StoreError> {
    let existing = state
        .store
        .agent_contract_observation(&turn.observation_id)
        .await?;
    if existing.outcome == AgentContractOutcome::Success.as_str() {
        return Ok(());
    }
    state
        .store
        .finalize_agent_contract_observation(turn.completion(
            workspace_id,
            AgentContractOutcome::Error,
            reason_code,
            session_id,
        ))
        .await?;
    Ok(())
}

pub(crate) fn agent_error_code(error: &AgentError) -> &'static str {
    match error {
        AgentError::Io(_) => "AGENT_IO_ERROR",
        AgentError::Json(_) => "AGENT_OUTPUT_JSON_ERROR",
        AgentError::Graph(_) => "AGENT_OUTPUT_GRAPH_ERROR",
        AgentError::PathOutsideSession(_) => "AGENT_OUTPUT_PATH_ERROR",
        AgentError::InvalidOutputFile { .. } => "AGENT_OUTPUT_INVALID",
        AgentError::InvalidMode { .. } => "AGENT_MODE_ERROR",
        AgentError::ProposalRetryExhausted { .. } => "AGENT_RETRY_EXHAUSTED",
        AgentError::Runtime(_) => "AGENT_RUNTIME_ERROR",
    }
}

pub(crate) fn contract_mode(use_intent_contract: bool) -> AgentContractMode {
    if use_intent_contract {
        AgentContractMode::Intent
    } else {
        AgentContractMode::Legacy
    }
}

fn read_optional_identity(name: &'static str, max_len: usize) -> Result<Option<String>, String> {
    match std::env::var(name) {
        Ok(value) if valid_identity(&value, max_len) => Ok(Some(value)),
        Ok(_) => Err(format!(
            "{name} must be 1-{max_len} ASCII letters, digits, '.', '-' or '_'"
        )),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{name} must be valid UTF-8")),
    }
}

fn valid_identity(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::{
        Json,
        extract::{Path, State},
    };
    use helixflow_agent::{
        AgentSessionRequest, ValidatedAgentIntent, ValidatedAgentProposal, ValidatedAgentReply,
    };
    use helixflow_compiler::IntentPlan;
    use helixflow_store::AgentContractObservationRecord;

    use super::*;
    use crate::app_state::WorkbenchAgent;
    use crate::workbench_message::{WorkspaceMessageRequest, post_workspace_message};
    use crate::workbench_message_tests::{sample_graph, state_with_workspace};

    #[test]
    fn identity_validation_rejects_paths_urls_whitespace_and_empty_values() {
        assert!(valid_identity("v0.2.0", 64));
        assert!(valid_identity("abc123_feature-1", 64));
        assert!(!valid_identity("", 64));
        assert!(!valid_identity("v0.2.0 token", 64));
        assert!(!valid_identity("https://release.invalid", 64));
        assert!(!valid_identity("/tmp/build", 64));
        assert!(!valid_identity(&"a".repeat(65), 64));
    }

    async fn observations(
        state: &AppState,
        workspace_id: &str,
    ) -> Vec<AgentContractObservationRecord> {
        sqlx::query_as(
            r#"
            SELECT id, workspace_id, user_message_id, session_id, contract_mode, outcome,
                   reason_code, release_id, build_revision, started_at, completed_at
            FROM agent_contract_observations
            WHERE workspace_id = ?
            ORDER BY started_at, id
            "#,
        )
        .bind(workspace_id)
        .fetch_all(state.store.pool())
        .await
        .expect("observations")
    }

    async fn post_graph_edit(
        state: AppState,
        workspace_id: String,
        version_id: String,
    ) -> Result<(), crate::api_error::ApiError> {
        post_workspace_message(
            Path(workspace_id),
            State(state),
            Json(WorkspaceMessageRequest {
                base_version_id: version_id,
                user_message: "创建一个 workflow".to_owned(),
                graph: sample_graph(),
                canvas_context: None,
            }),
        )
        .await
        .map(|_| ())
    }

    #[tokio::test]
    async fn legacy_success_records_release_attributed_terminal_observation() {
        let (mut state, workspace_id, version_id, _dir) = state_with_workspace().await;
        state.agent_contract_attribution = AgentContractAttribution {
            release_id: Some("v0.2.0".to_owned()),
            build_revision: Some("abc123".to_owned()),
        };

        post_graph_edit(state.clone(), workspace_id.clone(), version_id)
            .await
            .expect("legacy success");

        let rows = observations(&state, &workspace_id).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].contract_mode, "legacy");
        assert_eq!(rows[0].outcome, "success");
        assert_eq!(
            rows[0].reason_code.as_deref(),
            Some("LEGACY_PROPOSAL_APPLIED")
        );
        assert_eq!(rows[0].release_id.as_deref(), Some("v0.2.0"));
        assert_eq!(rows[0].build_revision.as_deref(), Some("abc123"));
        assert!(rows[0].session_id.is_some());
        assert!(rows[0].completed_at.is_some());
    }

    #[tokio::test]
    async fn agent_runtime_error_is_persisted_before_api_error_returns() {
        let (mut state, workspace_id, version_id, _dir) = state_with_workspace().await;
        state.agent = Arc::new(FailingGraphAgent);

        let error = post_graph_edit(state.clone(), workspace_id.clone(), version_id)
            .await
            .expect_err("agent error");
        assert_eq!(error.status, axum::http::StatusCode::BAD_GATEWAY);

        let rows = observations(&state, &workspace_id).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].contract_mode, "legacy");
        assert_eq!(rows[0].outcome, "error");
        assert_eq!(rows[0].reason_code.as_deref(), Some("AGENT_RUNTIME_ERROR"));
        assert!(
            !format!("{rows:?}").contains("sk-secret"),
            "raw agent error must not be persisted"
        );
    }

    #[tokio::test]
    async fn intent_success_clarify_and_compile_error_have_distinct_outcomes() {
        let cases = [
            (
                intent_plan(serde_json::json!({
                    "intentVersion": "1",
                    "topology": "linear",
                    "stages": [{
                        "stageId": "image",
                        "capabilityId": "text_to_image",
                        "requestedModel": "Nano Banana",
                        "inputFrom": [],
                        "params": {"prompt": "a product image"}
                    }],
                    "outputStageIds": ["image"]
                })),
                "success",
                "INTENT_COMPILED",
            ),
            (
                intent_plan(serde_json::json!({
                    "intentVersion": "1",
                    "topology": "linear",
                    "stages": [{
                        "stageId": "image",
                        "capabilityId": "text_to_image",
                        "inputFrom": [],
                        "params": {}
                    }],
                    "outputStageIds": ["image"]
                })),
                "clarify",
                "REQUIRED_INPUT_MISSING",
            ),
            (
                intent_plan(serde_json::json!({
                    "intentVersion": "1",
                    "topology": "linear",
                    "stages": [{
                        "stageId": "image",
                        "capabilityId": "text_to_image",
                        "inputFrom": [],
                        "params": {"prompt": "a product image"}
                    }],
                    "outputStageIds": ["unknown"]
                })),
                "error",
                "INTENT_INVALID",
            ),
        ];

        for (intent, expected_outcome, expected_code) in cases {
            let (mut state, workspace_id, version_id, _dir) = state_with_workspace().await;
            state.agent = Arc::new(ScriptedIntentAgent { intent });
            state.use_intent_contract = true;
            state.provider_registry = configured_atlas_registry();

            let result = post_graph_edit(state.clone(), workspace_id.clone(), version_id).await;
            if expected_outcome == "error" {
                assert!(result.is_err());
            } else {
                result.expect("non-error intent outcome");
            }
            let rows = observations(&state, &workspace_id).await;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].contract_mode, "intent");
            assert_eq!(rows[0].outcome, expected_outcome);
            assert_eq!(rows[0].reason_code.as_deref(), Some(expected_code));
        }
    }

    #[tokio::test]
    async fn chat_turn_does_not_enter_contract_evidence_denominator() {
        let (state, workspace_id, version_id, _dir) = state_with_workspace().await;
        let _response = post_workspace_message(
            Path(workspace_id.clone()),
            State(state.clone()),
            Json(WorkspaceMessageRequest {
                base_version_id: version_id,
                user_message: "你好".to_owned(),
                graph: sample_graph(),
                canvas_context: None,
            }),
        )
        .await
        .expect("chat");

        assert!(observations(&state, &workspace_id).await.is_empty());
    }

    fn intent_plan(value: serde_json::Value) -> IntentPlan {
        serde_json::from_value(value).expect("intent")
    }

    fn configured_atlas_registry() -> helixflow_gateway::ProviderRegistry {
        let atlas = helixflow_gateway::RuntimeProvider::Atlas(
            helixflow_gateway::AtlasProvider::new(helixflow_gateway::ApiProviderConfig::atlas(
                "test-key".to_owned(),
                "https://atlas.invalid/v1".to_owned(),
            )),
        );
        helixflow_gateway::ProviderRegistry::new("atlas", vec![atlas])
    }

    struct FailingGraphAgent;

    #[async_trait]
    impl WorkbenchAgent for FailingGraphAgent {
        async fn answer_chat(
            &self,
            _request: AgentSessionRequest,
        ) -> Result<ValidatedAgentReply, AgentError> {
            Err(AgentError::Runtime("not used".to_owned()))
        }

        async fn propose_graph_change(
            &self,
            _request: AgentSessionRequest,
        ) -> Result<ValidatedAgentProposal, AgentError> {
            Err(AgentError::Runtime(
                "provider failed with sk-secret-never-persist".to_owned(),
            ))
        }
    }

    struct ScriptedIntentAgent {
        intent: IntentPlan,
    }

    #[async_trait]
    impl WorkbenchAgent for ScriptedIntentAgent {
        async fn answer_chat(
            &self,
            _request: AgentSessionRequest,
        ) -> Result<ValidatedAgentReply, AgentError> {
            Err(AgentError::Runtime("not used".to_owned()))
        }

        async fn propose_graph_change(
            &self,
            _request: AgentSessionRequest,
        ) -> Result<ValidatedAgentProposal, AgentError> {
            Err(AgentError::Runtime("legacy path not expected".to_owned()))
        }

        async fn propose_intent(
            &self,
            request: AgentSessionRequest,
        ) -> Result<ValidatedAgentIntent, AgentError> {
            Ok(ValidatedAgentIntent {
                session_id: format!("{}_intent", request.workspace_id),
                agent_logs: Vec::new(),
                intent: self.intent.clone(),
            })
        }
    }
}
