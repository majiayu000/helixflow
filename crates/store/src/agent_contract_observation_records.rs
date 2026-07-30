use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, Transaction};

use super::{MessageRecord, NewMessage, Store, StoreError, StoreResult, new_id};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentContractMode {
    Intent,
    Legacy,
}

impl AgentContractMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Intent => "intent",
            Self::Legacy => "legacy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentContractOutcome {
    Started,
    Success,
    Clarify,
    Error,
}

impl AgentContractOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Success => "success",
            Self::Clarify => "clarify",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct AgentContractObservationRecord {
    pub id: String,
    pub workspace_id: String,
    pub user_message_id: String,
    pub session_id: Option<String>,
    pub contract_mode: String,
    pub outcome: String,
    pub reason_code: Option<String>,
    pub release_id: Option<String>,
    pub build_revision: Option<String>,
    pub started_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewAgentContractObservation<'a> {
    pub user_message: NewMessage<'a>,
    pub contract_mode: AgentContractMode,
    pub release_id: Option<&'a str>,
    pub build_revision: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct CompleteAgentContractObservation<'a> {
    pub observation_id: &'a str,
    pub workspace_id: &'a str,
    pub contract_mode: AgentContractMode,
    pub outcome: AgentContractOutcome,
    pub reason_code: &'a str,
    pub session_id: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedAgentContractObservation {
    pub message: MessageRecord,
    pub observation: AgentContractObservationRecord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClarifiedAgentContractObservation {
    pub message: MessageRecord,
    pub observation: AgentContractObservationRecord,
}

#[derive(Debug, Clone)]
pub struct AgentContractEvidenceFilter<'a> {
    pub since: &'a str,
    pub until: &'a str,
    pub release_id: Option<&'a str>,
    pub build_revision: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct AgentContractEvidenceGroup {
    pub contract_mode: String,
    pub outcome: String,
    pub reason_code: Option<String>,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentContractEvidenceAggregate {
    pub groups: Vec<AgentContractEvidenceGroup>,
    pub unattributed: i64,
    pub in_flight: i64,
}

impl Store {
    pub async fn create_graph_edit_message_with_observation(
        &self,
        input: NewAgentContractObservation<'_>,
    ) -> StoreResult<StartedAgentContractObservation> {
        validate_user_message(&input.user_message)?;
        validate_optional_identity("INVALID_RELEASE_ID", input.release_id, 64)?;
        validate_optional_identity("INVALID_BUILD_REVISION", input.build_revision, 128)?;

        let message_id = new_id("msg");
        let observation_id = new_id("aco");
        let mut tx = self.pool().begin_with("BEGIN IMMEDIATE").await?;
        insert_message(&mut tx, &message_id, &input.user_message).await?;
        sqlx::query(
            r#"
            INSERT INTO agent_contract_observations (
                id, workspace_id, user_message_id, contract_mode, outcome,
                release_id, build_revision, started_at
            )
            VALUES (?, ?, ?, ?, 'started', ?, ?, current_timestamp)
            "#,
        )
        .bind(&observation_id)
        .bind(input.user_message.workspace_id)
        .bind(&message_id)
        .bind(input.contract_mode.as_str())
        .bind(input.release_id)
        .bind(input.build_revision)
        .execute(&mut *tx)
        .await?;

        let message = message_in_tx(&mut tx, &message_id).await?;
        let observation = observation_in_tx(&mut tx, &observation_id).await?;
        tx.commit().await?;
        Ok(StartedAgentContractObservation {
            message,
            observation,
        })
    }

    pub async fn agent_contract_observation(
        &self,
        observation_id: &str,
    ) -> StoreResult<AgentContractObservationRecord> {
        Ok(sqlx::query_as(
            r#"
            SELECT id, workspace_id, user_message_id, session_id, contract_mode, outcome,
                   reason_code, release_id, build_revision, started_at, completed_at
            FROM agent_contract_observations
            WHERE id = ?
            "#,
        )
        .bind(observation_id)
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn finalize_agent_contract_observation(
        &self,
        input: CompleteAgentContractObservation<'_>,
    ) -> StoreResult<AgentContractObservationRecord> {
        let mut tx = self.pool().begin_with("BEGIN IMMEDIATE").await?;
        let observation = complete_observation_in_tx(&mut tx, &input).await?;
        tx.commit().await?;
        Ok(observation)
    }

    pub async fn create_clarification_and_finalize_observation(
        &self,
        message: NewMessage<'_>,
        completion: CompleteAgentContractObservation<'_>,
    ) -> StoreResult<ClarifiedAgentContractObservation> {
        if message.workspace_id != completion.workspace_id
            || message.role != "agent"
            || message.kind != "clarify"
            || completion.outcome != AgentContractOutcome::Clarify
        {
            return Err(StoreError::AgentContractObservationInvariant {
                code: "INVALID_CLARIFICATION_COMPLETION",
            });
        }
        let message_id = new_id("msg");
        let mut tx = self.pool().begin_with("BEGIN IMMEDIATE").await?;
        insert_message(&mut tx, &message_id, &message).await?;
        let observation = complete_observation_in_tx(&mut tx, &completion).await?;
        let message = message_in_tx(&mut tx, &message_id).await?;
        tx.commit().await?;
        Ok(ClarifiedAgentContractObservation {
            message,
            observation,
        })
    }

    pub async fn finalize_interrupted_agent_contract_observations(&self) -> StoreResult<u64> {
        Ok(sqlx::query(
            r#"
            UPDATE agent_contract_observations
            SET outcome = 'error',
                reason_code = 'PROCESS_INTERRUPTED',
                completed_at = current_timestamp
            WHERE outcome = 'started'
            "#,
        )
        .execute(self.pool())
        .await?
        .rows_affected())
    }

    pub async fn agent_contract_evidence(
        &self,
        filter: AgentContractEvidenceFilter<'_>,
    ) -> StoreResult<AgentContractEvidenceAggregate> {
        if filter.since >= filter.until {
            return Err(StoreError::AgentContractObservationInvariant {
                code: "INVALID_EVIDENCE_WINDOW",
            });
        }
        validate_optional_identity("INVALID_RELEASE_ID", filter.release_id, 64)?;
        validate_optional_identity("INVALID_BUILD_REVISION", filter.build_revision, 128)?;
        let groups = sqlx::query_as(
            r#"
            SELECT contract_mode, outcome, reason_code, COUNT(*) AS count
            FROM agent_contract_observations
            WHERE started_at >= ? AND started_at < ?
              AND (? IS NULL OR release_id = ?)
              AND (? IS NULL OR build_revision = ?)
            GROUP BY contract_mode, outcome, reason_code
            ORDER BY contract_mode, outcome, reason_code
            "#,
        )
        .bind(filter.since)
        .bind(filter.until)
        .bind(filter.release_id)
        .bind(filter.release_id)
        .bind(filter.build_revision)
        .bind(filter.build_revision)
        .fetch_all(self.pool())
        .await?;
        let unattributed = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM agent_contract_observations
            WHERE started_at >= ? AND started_at < ?
              AND (release_id IS NULL OR build_revision IS NULL)
            "#,
        )
        .bind(filter.since)
        .bind(filter.until)
        .fetch_one(self.pool())
        .await?;
        let in_flight = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM agent_contract_observations
            WHERE started_at >= ? AND started_at < ? AND outcome = 'started'
            "#,
        )
        .bind(filter.since)
        .bind(filter.until)
        .fetch_one(self.pool())
        .await?;
        Ok(AgentContractEvidenceAggregate {
            groups,
            unattributed,
            in_flight,
        })
    }
}

pub(crate) async fn complete_observation_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: &CompleteAgentContractObservation<'_>,
) -> StoreResult<AgentContractObservationRecord> {
    validate_completion(input)?;
    let updated = sqlx::query(
        r#"
        UPDATE agent_contract_observations
        SET outcome = ?, reason_code = ?, session_id = ?, completed_at = current_timestamp
        WHERE id = ? AND workspace_id = ? AND contract_mode = ? AND outcome = 'started'
        "#,
    )
    .bind(input.outcome.as_str())
    .bind(input.reason_code)
    .bind(input.session_id)
    .bind(input.observation_id)
    .bind(input.workspace_id)
    .bind(input.contract_mode.as_str())
    .execute(&mut **tx)
    .await?;
    let existing = observation_in_tx(tx, input.observation_id).await?;
    if updated.rows_affected() == 1
        || (existing.workspace_id == input.workspace_id
            && existing.contract_mode == input.contract_mode.as_str()
            && existing.outcome == input.outcome.as_str()
            && existing.reason_code.as_deref() == Some(input.reason_code)
            && existing.session_id.as_deref() == input.session_id)
    {
        return Ok(existing);
    }
    Err(StoreError::AgentContractObservationInvariant {
        code: "TERMINAL_COMPLETION_CONFLICT",
    })
}

async fn insert_message(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    input: &NewMessage<'_>,
) -> StoreResult<()> {
    sqlx::query(
        r#"
        INSERT INTO messages (
            id, workspace_id, role, text, kind, ref_id, attachment_ids_json, created_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, current_timestamp)
        "#,
    )
    .bind(id)
    .bind(input.workspace_id)
    .bind(input.role)
    .bind(input.text)
    .bind(input.kind)
    .bind(input.ref_id)
    .bind(input.attachment_ids_json)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn message_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    message_id: &str,
) -> StoreResult<MessageRecord> {
    Ok(sqlx::query_as(
        r#"
        SELECT id, workspace_id, role, text, kind, ref_id, attachment_ids_json, created_at
        FROM messages
        WHERE id = ?
        "#,
    )
    .bind(message_id)
    .fetch_one(&mut **tx)
    .await?)
}

async fn observation_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    observation_id: &str,
) -> StoreResult<AgentContractObservationRecord> {
    Ok(sqlx::query_as(
        r#"
        SELECT id, workspace_id, user_message_id, session_id, contract_mode, outcome,
               reason_code, release_id, build_revision, started_at, completed_at
        FROM agent_contract_observations
        WHERE id = ?
        "#,
    )
    .bind(observation_id)
    .fetch_one(&mut **tx)
    .await?)
}

fn validate_user_message(message: &NewMessage<'_>) -> StoreResult<()> {
    if message.role == "user" && message.kind == "text" && message.text.is_some() {
        return Ok(());
    }
    Err(StoreError::AgentContractObservationInvariant {
        code: "INVALID_USER_MESSAGE",
    })
}

fn validate_completion(input: &CompleteAgentContractObservation<'_>) -> StoreResult<()> {
    if input.outcome == AgentContractOutcome::Started
        || !valid_stable_code(input.reason_code, 128)
        || input.observation_id.is_empty()
        || input.workspace_id.is_empty()
    {
        return Err(StoreError::AgentContractObservationInvariant {
            code: "INVALID_TERMINAL_COMPLETION",
        });
    }
    Ok(())
}

fn validate_optional_identity(
    code: &'static str,
    value: Option<&str>,
    max_len: usize,
) -> StoreResult<()> {
    if value.is_none_or(|value| valid_identity(value, max_len)) {
        return Ok(());
    }
    Err(StoreError::AgentContractObservationInvariant { code })
}

fn valid_identity(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn valid_stable_code(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}
