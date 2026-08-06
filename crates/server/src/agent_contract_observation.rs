use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{Query, State},
};
use helixflow_agent::AgentError;
use helixflow_store::{
    AgentContractEvidenceFilter, AgentContractMode, AgentContractOutcome,
    CompleteAgentContractObservation, StoreError,
};
use serde::{Deserialize, Serialize};

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::version_migration_routes::{VersionMigrationStatus, assess_current_version};

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
        AgentError::InvalidPromptContext(_) => "AGENT_PROMPT_CONTEXT_INVALID",
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AgentContractEvidenceQuery {
    since: String,
    until: String,
    release_id: Option<String>,
    build_revision: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentContractEvidenceResponse {
    window: EvidenceWindow,
    filter: EvidenceAttributionFilter,
    intent: ContractCounts,
    legacy: LegacyContractCounts,
    attribution: EvidenceAttribution,
    migration: MigrationEvidence,
    limitations: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EvidenceWindow {
    since: String,
    until: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EvidenceAttributionFilter {
    release_id: Option<String>,
    build_revision: Option<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContractCounts {
    total: i64,
    success: i64,
    clarify: i64,
    error: i64,
    success_rate: Option<f64>,
    clarify_reasons: BTreeMap<String, i64>,
    error_reasons: BTreeMap<String, i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyContractCounts {
    #[serde(flatten)]
    counts: ContractCounts,
    rollback_events: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EvidenceAttribution {
    unattributed: i64,
    in_flight: i64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct MigrationEvidence {
    total_current_versions: i64,
    already_migrated: i64,
    migratable: i64,
    needs_resolution: i64,
    failed: i64,
    missing_or_currentless: i64,
    approved_isolation: i64,
    complete: bool,
}

pub(crate) async fn agent_contract_evidence(
    State(state): State<AppState>,
    Query(query): Query<AgentContractEvidenceQuery>,
) -> Result<Json<AgentContractEvidenceResponse>, ApiError> {
    let since = parse_utc_seconds("since", &query.since)?;
    let until = parse_utc_seconds("until", &query.until)?;
    if since >= until {
        return Err(ApiError::bad_request("since must be earlier than until"));
    }
    validate_query_identity("releaseId", query.release_id.as_deref(), 64)?;
    validate_query_identity("buildRevision", query.build_revision.as_deref(), 128)?;
    let aggregate = state
        .store
        .agent_contract_evidence(AgentContractEvidenceFilter {
            since: &since,
            until: &until,
            release_id: query.release_id.as_deref(),
            build_revision: query.build_revision.as_deref(),
        })
        .await
        .map_err(ApiError::store)?;
    let mut intent = ContractCounts::default();
    let mut legacy_counts = ContractCounts::default();
    for group in aggregate.groups {
        let counts = match group.contract_mode.as_str() {
            "intent" => &mut intent,
            "legacy" => &mut legacy_counts,
            _ => {
                return Err(ApiError::server_error(
                    "stored agent contract mode is invalid",
                ));
            }
        };
        match group.outcome.as_str() {
            "started" => {}
            "success" => counts.success += group.count,
            "clarify" => {
                counts.clarify += group.count;
                add_reason(&mut counts.clarify_reasons, group.reason_code, group.count)?;
            }
            "error" => {
                counts.error += group.count;
                add_reason(&mut counts.error_reasons, group.reason_code, group.count)?;
            }
            _ => {
                return Err(ApiError::server_error(
                    "stored agent contract outcome is invalid",
                ));
            }
        }
    }
    finalize_counts(&mut intent);
    finalize_counts(&mut legacy_counts);
    let rollback_events = legacy_counts.total;
    let migration = collect_migration_evidence(&state).await?;
    let mut limitations = Vec::new();
    if intent.total == 0 {
        limitations.push("ZERO_INTENT_SAMPLES");
    }
    if aggregate.unattributed > 0 {
        limitations.push("UNATTRIBUTED_SAMPLES");
    }
    if aggregate.in_flight > 0 {
        limitations.push("IN_FLIGHT_SAMPLES");
    }
    if !migration.complete {
        limitations.push("MIGRATION_INCOMPLETE");
    }
    limitations.push("APPROVED_ISOLATION_UNSUPPORTED");

    Ok(Json(AgentContractEvidenceResponse {
        window: EvidenceWindow {
            since: query.since,
            until: query.until,
        },
        filter: EvidenceAttributionFilter {
            release_id: query.release_id,
            build_revision: query.build_revision,
        },
        intent,
        legacy: LegacyContractCounts {
            counts: legacy_counts,
            rollback_events,
        },
        attribution: EvidenceAttribution {
            unattributed: aggregate.unattributed,
            in_flight: aggregate.in_flight,
        },
        migration,
        limitations,
    }))
}

fn finalize_counts(counts: &mut ContractCounts) {
    counts.total = counts.success + counts.clarify + counts.error;
    counts.success_rate = (counts.total > 0).then(|| counts.success as f64 / counts.total as f64);
}

fn add_reason(
    reasons: &mut BTreeMap<String, i64>,
    reason_code: Option<String>,
    count: i64,
) -> Result<(), ApiError> {
    let reason_code =
        reason_code.ok_or_else(|| ApiError::server_error("terminal observation has no reason"))?;
    *reasons.entry(reason_code).or_default() += count;
    Ok(())
}

async fn collect_migration_evidence(state: &AppState) -> Result<MigrationEvidence, ApiError> {
    let mut evidence = MigrationEvidence::default();
    for workspace in state.store.workspaces().await.map_err(ApiError::store)? {
        let Some(version_id) = workspace.cur_version_id else {
            evidence.missing_or_currentless += 1;
            continue;
        };
        evidence.total_current_versions += 1;
        match assess_current_version(state, &workspace.id, &version_id, false).await {
            Ok(assessment) => match assessment.report.status() {
                VersionMigrationStatus::AlreadyMigrated => evidence.already_migrated += 1,
                VersionMigrationStatus::Migratable => evidence.migratable += 1,
                VersionMigrationStatus::NeedsResolution => evidence.needs_resolution += 1,
                VersionMigrationStatus::Failed => evidence.failed += 1,
            },
            Err(_) => evidence.failed += 1,
        }
    }
    evidence.complete = evidence.total_current_versions > 0
        && evidence.already_migrated == evidence.total_current_versions
        && evidence.missing_or_currentless == 0;
    Ok(evidence)
}

fn parse_utc_seconds(name: &str, value: &str) -> Result<String, ApiError> {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return Err(ApiError::bad_request(format!(
            "{name} must use YYYY-MM-DDTHH:MM:SSZ"
        )));
    }
    let year = parse_digits(name, bytes, 0, 4)?;
    let month = parse_digits(name, bytes, 5, 2)?;
    let day = parse_digits(name, bytes, 8, 2)?;
    let hour = parse_digits(name, bytes, 11, 2)?;
    let minute = parse_digits(name, bytes, 14, 2)?;
    let second = parse_digits(name, bytes, 17, 2)?;
    if year == 0
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err(ApiError::bad_request(format!(
            "{name} is not a valid UTC timestamp"
        )));
    }
    Ok(format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}"
    ))
}

fn parse_digits(name: &str, bytes: &[u8], start: usize, len: usize) -> Result<u32, ApiError> {
    let slice = &bytes[start..start + len];
    if !slice.iter().all(u8::is_ascii_digit) {
        return Err(ApiError::bad_request(format!(
            "{name} is not a valid UTC timestamp"
        )));
    }
    Ok(slice
        .iter()
        .fold(0_u32, |value, digit| value * 10 + u32::from(digit - b'0')))
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        _ => 31,
    }
}

fn validate_query_identity(
    name: &str,
    value: Option<&str>,
    max_len: usize,
) -> Result<(), ApiError> {
    if value.is_none_or(|value| valid_identity(value, max_len)) {
        return Ok(());
    }
    Err(ApiError::bad_request(format!("{name} is invalid")))
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

    #[tokio::test]
    async fn evidence_endpoint_filters_release_and_rechecks_current_migration_state() {
        let intent = intent_plan(serde_json::json!({
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
        }));
        let (mut state, workspace_id, version_id, _dir) = state_with_workspace().await;
        state.agent = Arc::new(ScriptedIntentAgent { intent });
        state.use_intent_contract = true;
        state.provider_registry = configured_atlas_registry();
        state.agent_contract_attribution = AgentContractAttribution {
            release_id: Some("v0.2.0".to_owned()),
            build_revision: Some("abc123".to_owned()),
        };
        post_graph_edit(state.clone(), workspace_id, version_id)
            .await
            .expect("intent success");

        let response = agent_contract_evidence(
            State(state),
            Query(AgentContractEvidenceQuery {
                since: "2000-01-01T00:00:00Z".to_owned(),
                until: "2100-01-01T00:00:00Z".to_owned(),
                release_id: Some("v0.2.0".to_owned()),
                build_revision: Some("abc123".to_owned()),
            }),
        )
        .await
        .expect("evidence")
        .0;

        assert_eq!(response.intent.total, 1);
        assert_eq!(response.intent.success, 1);
        assert_eq!(response.intent.success_rate, Some(1.0));
        assert_eq!(response.legacy.rollback_events, 0);
        assert_eq!(response.attribution.unattributed, 0);
        assert!(response.migration.complete);
        assert_eq!(response.migration.already_migrated, 1);
        assert_eq!(response.limitations, vec!["APPROVED_ISOLATION_UNSUPPORTED"]);
    }

    #[test]
    fn evidence_time_parser_is_strict_and_calendar_aware() {
        assert_eq!(
            parse_utc_seconds("since", "2026-07-31T12:34:56Z").expect("time"),
            "2026-07-31 12:34:56"
        );
        assert!(parse_utc_seconds("since", "2026-02-29T00:00:00Z").is_err());
        assert!(parse_utc_seconds("since", "2026-07-31 12:34:56Z").is_err());
        assert!(parse_utc_seconds("since", "https://invalid").is_err());
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
