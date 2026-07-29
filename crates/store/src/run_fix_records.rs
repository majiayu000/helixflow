use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Row};

use super::{NewRun, RunEventRecord, RunRecord, Store, StoreError, StoreResult, new_id};

macro_rules! fix_attempt_sql {
    ($suffix:literal) => {
        concat!(
            r#"
            SELECT id, operation_id, workspace_id, chain_id, root_run_id, source_run_id,
                   attempt_index, max_attempts, source_version_id,
                   expected_runtime_provider_id, effective_provider_id,
                   expected_recovery_scope_fingerprint, provider_catalog_fingerprint,
                   state, proposal_id, target_version_id, child_run_id, reason_code,
                   error_summary, created_at, updated_at, finished_at
            FROM run_fix_attempts
            "#,
            $suffix
        )
    };
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, FromRow)]
pub struct RunRepairChainRecord {
    pub id: String,
    pub workspace_id: String,
    pub root_run_id: String,
    pub provenance: String,
    pub sweep_group_id: Option<String>,
    pub recommended_run_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, FromRow)]
pub struct RunFixAttemptRecord {
    pub id: String,
    pub operation_id: String,
    pub workspace_id: String,
    pub chain_id: String,
    pub root_run_id: String,
    pub source_run_id: String,
    pub attempt_index: i64,
    pub max_attempts: i64,
    pub source_version_id: String,
    pub expected_runtime_provider_id: Option<String>,
    pub effective_provider_id: String,
    pub expected_recovery_scope_fingerprint: String,
    pub provider_catalog_fingerprint: String,
    pub state: String,
    pub proposal_id: Option<String>,
    pub target_version_id: Option<String>,
    pub child_run_id: Option<String>,
    pub reason_code: Option<String>,
    pub error_summary: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, FromRow)]
pub struct RunFixEventOutboxRecord {
    pub id: String,
    pub dedupe_key: String,
    pub run_id: String,
    pub event_name: String,
    pub data_json: String,
    pub state: String,
    pub event_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct RunFixSnapshot<'a> {
    pub expected_runtime_provider_id: Option<&'a str>,
    pub effective_provider_id: &'a str,
    pub expected_recovery_scope_fingerprint: &'a str,
    pub provider_catalog_fingerprint: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunFixClaim {
    Claimed(Box<RunFixAttemptRecord>),
    Exhausted,
    NotEligible,
}

impl Store {
    pub async fn create_agent_run_with_repair_chain(
        &self,
        input: NewRun<'_>,
    ) -> StoreResult<(RunRecord, RunRepairChainRecord)> {
        if input.trigger != "agent" {
            return Err(StoreError::RecoveryInvariant {
                operation: "create_agent_run_with_repair_chain",
                message: "repair-chain root must be an agent run".to_owned(),
            });
        }
        let mut tx = self.pool().begin().await?;
        let actual_workspace_id: Option<String> =
            sqlx::query_scalar("SELECT workspace_id FROM versions WHERE id = ?")
                .bind(input.version_id)
                .fetch_optional(&mut *tx)
                .await?;
        if actual_workspace_id.as_deref() != Some(input.workspace_id) {
            return Err(StoreError::RunVersionMismatch {
                workspace_id: input.workspace_id.to_owned(),
                version_id: input.version_id.to_owned(),
                actual_workspace_id,
            });
        }
        let run_id = new_id("run");
        sqlx::query(
            r#"
            INSERT INTO runs (
                id, workspace_id, version_id, group_id, label, trigger, plan_json,
                estimate_json, status, force_rerun, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0, current_timestamp)
            "#,
        )
        .bind(&run_id)
        .bind(input.workspace_id)
        .bind(input.version_id)
        .bind(input.group_id)
        .bind(input.label)
        .bind(input.trigger)
        .bind(input.plan_json)
        .bind(input.estimate_json)
        .bind(input.status)
        .execute(&mut *tx)
        .await?;
        let chain_id = new_id("fixchain");
        sqlx::query(
            r#"
            INSERT INTO run_repair_chains (
                id, workspace_id, root_run_id, provenance, created_at
            )
            VALUES (?, ?, ?, 'agent', current_timestamp)
            "#,
        )
        .bind(&chain_id)
        .bind(input.workspace_id)
        .bind(&run_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO run_repair_chain_runs (run_id, chain_id, relation, created_at)
            VALUES (?, ?, 'root', current_timestamp)
            "#,
        )
        .bind(&run_id)
        .bind(&chain_id)
        .execute(&mut *tx)
        .await?;
        let run = run_in_tx(&mut tx, &run_id).await?;
        let chain = repair_chain_in_tx(&mut tx, &chain_id).await?;
        tx.commit().await?;
        Ok((run, chain))
    }

    pub async fn create_recommended_sweep_repair_chain(
        &self,
        run_id: &str,
        group_id: &str,
    ) -> StoreResult<RunRepairChainRecord> {
        let mut tx = self.pool().begin().await?;
        sqlx::query("UPDATE runs SET status = status WHERE id = ?")
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        let run = run_in_tx(&mut tx, run_id).await?;
        if run.trigger != "sweep"
            || run.group_id.as_deref() != Some(group_id)
            || run.status != "waiting_confirmation"
        {
            return Err(StoreError::RecoveryInvariant {
                operation: "create_recommended_sweep_repair_chain",
                message: "recommended sweep root does not match its confirmed group".to_owned(),
            });
        }
        let chain_id = new_id("fixchain");
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO run_repair_chains (
                id, workspace_id, root_run_id, provenance,
                sweep_group_id, recommended_run_id, created_at
            )
            VALUES (?, ?, ?, 'recommended_sweep', ?, ?, current_timestamp)
            "#,
        )
        .bind(&chain_id)
        .bind(&run.workspace_id)
        .bind(run_id)
        .bind(group_id)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        let chain = sqlx::query_as::<_, RunRepairChainRecord>(
            r#"
            SELECT id, workspace_id, root_run_id, provenance, sweep_group_id,
                   recommended_run_id, created_at
            FROM run_repair_chains WHERE root_run_id = ?
            "#,
        )
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?;
        if chain.provenance != "recommended_sweep"
            || chain.sweep_group_id.as_deref() != Some(group_id)
            || chain.recommended_run_id.as_deref() != Some(run_id)
        {
            return Err(StoreError::RecoveryInvariant {
                operation: "create_recommended_sweep_repair_chain",
                message: "recommended sweep chain identity changed".to_owned(),
            });
        }
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO run_repair_chain_runs (
                run_id, chain_id, relation, created_at
            )
            VALUES (?, ?, 'root', current_timestamp)
            "#,
        )
        .bind(run_id)
        .bind(&chain.id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(chain)
    }

    pub async fn repair_chain_for_run(
        &self,
        run_id: &str,
    ) -> StoreResult<Option<RunRepairChainRecord>> {
        Ok(sqlx::query_as::<_, RunRepairChainRecord>(
            r#"
            SELECT chain.id, chain.workspace_id, chain.root_run_id, chain.provenance,
                   chain.sweep_group_id, chain.recommended_run_id, chain.created_at
            FROM run_repair_chain_runs AS member
            JOIN run_repair_chains AS chain ON chain.id = member.chain_id
            WHERE member.run_id = ?
            "#,
        )
        .bind(run_id)
        .fetch_optional(self.pool())
        .await?)
    }

    pub async fn claim_run_fix_attempt(
        &self,
        source_run_id: &str,
        max_attempts: u32,
        snapshot: RunFixSnapshot<'_>,
    ) -> StoreResult<RunFixClaim> {
        let mut tx = self.pool().begin().await?;
        sqlx::query(
            "UPDATE run_failure_continuations SET updated_at = updated_at WHERE run_id = ?",
        )
        .bind(source_run_id)
        .execute(&mut *tx)
        .await?;
        let continuation = sqlx::query(
            r#"
            SELECT state, chain_id FROM run_failure_continuations WHERE run_id = ?
            "#,
        )
        .bind(source_run_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(continuation) = continuation else {
            tx.rollback().await?;
            return Ok(RunFixClaim::NotEligible);
        };
        let state: String = continuation.try_get("state")?;
        let chain_id: Option<String> = continuation.try_get("chain_id")?;
        let Some(chain_id) = chain_id else {
            tx.rollback().await?;
            return Ok(RunFixClaim::NotEligible);
        };
        if !matches!(state.as_str(), "exhausted" | "fix_pending") {
            tx.rollback().await?;
            return Ok(RunFixClaim::NotEligible);
        }
        let chain = repair_chain_in_tx(&mut tx, &chain_id).await?;
        let source = run_in_tx(&mut tx, source_run_id).await?;
        if source.status != "failed" || source.workspace_id != chain.workspace_id {
            tx.rollback().await?;
            return Ok(RunFixClaim::NotEligible);
        }
        let consumed: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM run_fix_attempts WHERE chain_id = ?")
                .bind(&chain_id)
                .fetch_one(&mut *tx)
                .await?;
        if consumed >= i64::from(max_attempts) {
            finish_continuation_exhausted(
                &mut tx,
                source_run_id,
                &chain,
                consumed,
                max_attempts,
                "FIX_LIMIT_REACHED",
            )
            .await?;
            tx.commit().await?;
            return Ok(RunFixClaim::Exhausted);
        }
        let attempt_index = consumed + 1;
        let attempt_id = new_id("fix");
        let operation_id = format!("fix:{}:{attempt_index}", chain.root_run_id);
        sqlx::query(
            r#"
            INSERT INTO run_fix_attempts (
                id, operation_id, workspace_id, chain_id, root_run_id,
                source_run_id, attempt_index, max_attempts, source_version_id,
                expected_runtime_provider_id, effective_provider_id,
                expected_recovery_scope_fingerprint, provider_catalog_fingerprint,
                state, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'claimed',
                    current_timestamp, current_timestamp)
            "#,
        )
        .bind(&attempt_id)
        .bind(&operation_id)
        .bind(&chain.workspace_id)
        .bind(&chain.id)
        .bind(&chain.root_run_id)
        .bind(source_run_id)
        .bind(attempt_index)
        .bind(i64::from(max_attempts))
        .bind(&source.version_id)
        .bind(snapshot.expected_runtime_provider_id)
        .bind(snapshot.effective_provider_id)
        .bind(snapshot.expected_recovery_scope_fingerprint)
        .bind(snapshot.provider_catalog_fingerprint)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE run_failure_continuations
            SET state = 'fix_claimed', reason_code = NULL, updated_at = current_timestamp
            WHERE run_id = ? AND state IN ('exhausted', 'fix_pending')
            "#,
        )
        .bind(source_run_id)
        .execute(&mut *tx)
        .await?;
        insert_fix_outbox(
            &mut tx,
            &format!("{}:attempt:{attempt_index}", chain.id),
            source_run_id,
            "run.fix_attempt",
            &serde_json::json!({
                "operation_id": operation_id,
                "attempt": attempt_index,
                "max_attempts": max_attempts,
                "source_version_id": source.version_id,
            })
            .to_string(),
        )
        .await?;
        let attempt = fix_attempt_in_tx(&mut tx, &attempt_id).await?;
        tx.commit().await?;
        Ok(RunFixClaim::Claimed(Box::new(attempt)))
    }

    pub async fn mark_run_fix_agent_running(
        &self,
        attempt_id: &str,
    ) -> StoreResult<Option<RunFixAttemptRecord>> {
        let updated = sqlx::query(
            r#"
            UPDATE run_fix_attempts
            SET state = 'agent_running', updated_at = current_timestamp
            WHERE id = ? AND state = 'claimed'
            "#,
        )
        .bind(attempt_id)
        .execute(self.pool())
        .await?;
        if updated.rows_affected() == 0 {
            return self.run_fix_attempt(attempt_id).await;
        }
        self.run_fix_attempt(attempt_id).await
    }

    pub async fn fail_run_fix_attempt(
        &self,
        attempt_id: &str,
        reason_code: &str,
        error_summary: Option<&str>,
    ) -> StoreResult<Option<RunFixAttemptRecord>> {
        let mut tx = self.pool().begin().await?;
        sqlx::query("UPDATE run_fix_attempts SET updated_at = updated_at WHERE id = ?")
            .bind(attempt_id)
            .execute(&mut *tx)
            .await?;
        let attempt = fix_attempt_in_tx(&mut tx, attempt_id).await?;
        if matches!(attempt.state.as_str(), "failed" | "exhausted" | "cancelled") {
            tx.commit().await?;
            return Ok(Some(attempt));
        }
        if !matches!(
            attempt.state.as_str(),
            "claimed" | "agent_running" | "version_applied" | "child_preparing"
        ) {
            tx.rollback().await?;
            return Ok(None);
        }
        let failed_after_apply = matches!(
            attempt.state.as_str(),
            "version_applied" | "child_preparing"
        );
        sqlx::query(
            r#"
            UPDATE run_fix_attempts
            SET state = 'failed', reason_code = ?, error_summary = ?,
                finished_at = current_timestamp, updated_at = current_timestamp
            WHERE id = ? AND state IN (
                'claimed', 'agent_running', 'version_applied', 'child_preparing'
            )
            "#,
        )
        .bind(reason_code)
        .bind(error_summary)
        .bind(attempt_id)
        .execute(&mut *tx)
        .await?;
        let consumed: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM run_fix_attempts WHERE chain_id = ?")
                .bind(&attempt.chain_id)
                .fetch_one(&mut *tx)
                .await?;
        if failed_after_apply || consumed >= attempt.max_attempts {
            let chain = repair_chain_in_tx(&mut tx, &attempt.chain_id).await?;
            finish_continuation_exhausted(
                &mut tx,
                &attempt.source_run_id,
                &chain,
                consumed,
                u32::try_from(attempt.max_attempts).unwrap_or_default(),
                reason_code,
            )
            .await?;
        } else {
            sqlx::query(
                r#"
                UPDATE run_failure_continuations
                SET state = 'fix_pending', reason_code = ?, updated_at = current_timestamp
                WHERE run_id = ? AND state = 'fix_claimed'
                "#,
            )
            .bind(reason_code)
            .bind(&attempt.source_run_id)
            .execute(&mut *tx)
            .await?;
        }
        let updated = fix_attempt_in_tx(&mut tx, attempt_id).await?;
        tx.commit().await?;
        Ok(Some(updated))
    }

    pub async fn run_fix_attempt(
        &self,
        attempt_id: &str,
    ) -> StoreResult<Option<RunFixAttemptRecord>> {
        Ok(
            sqlx::query_as::<_, RunFixAttemptRecord>(fix_attempt_sql!(" WHERE id = ?"))
                .bind(attempt_id)
                .fetch_optional(self.pool())
                .await?,
        )
    }

    pub async fn pending_run_fix_outbox(&self) -> StoreResult<Vec<RunFixEventOutboxRecord>> {
        Ok(sqlx::query_as::<_, RunFixEventOutboxRecord>(
            r#"
            SELECT id, dedupe_key, run_id, event_name, data_json, state, event_id,
                   created_at, updated_at
            FROM run_fix_event_outbox
            WHERE state IN ('pending', 'event_persisted')
            ORDER BY created_at, id
            "#,
        )
        .fetch_all(self.pool())
        .await?)
    }

    pub async fn persist_run_fix_outbox_event(
        &self,
        outbox_id: &str,
    ) -> StoreResult<Option<RunEventRecord>> {
        let mut tx = self.pool().begin().await?;
        sqlx::query("UPDATE run_fix_event_outbox SET updated_at = updated_at WHERE id = ?")
            .bind(outbox_id)
            .execute(&mut *tx)
            .await?;
        let outbox = sqlx::query_as::<_, RunFixEventOutboxRecord>(
            r#"
            SELECT id, dedupe_key, run_id, event_name, data_json, state, event_id,
                   created_at, updated_at
            FROM run_fix_event_outbox WHERE id = ?
            "#,
        )
        .bind(outbox_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(outbox) = outbox else {
            tx.rollback().await?;
            return Ok(None);
        };
        if outbox.state == "broadcasted" {
            tx.commit().await?;
            return Ok(None);
        }
        let event_id = outbox.event_id.unwrap_or_else(|| new_id("evt"));
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO run_events (id, run_id, seq, ev, data_json, created_at)
            SELECT ?, ?, COALESCE(MAX(seq), 0) + 1, ?, ?, current_timestamp
            FROM run_events WHERE run_id = ?
            "#,
        )
        .bind(&event_id)
        .bind(&outbox.run_id)
        .bind(&outbox.event_name)
        .bind(&outbox.data_json)
        .bind(&outbox.run_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE run_fix_event_outbox
            SET state = 'event_persisted', event_id = ?, updated_at = current_timestamp
            WHERE id = ? AND state IN ('pending', 'event_persisted')
            "#,
        )
        .bind(&event_id)
        .bind(outbox_id)
        .execute(&mut *tx)
        .await?;
        let event = sqlx::query_as::<_, RunEventRecord>(
            r#"
            SELECT id, run_id, seq, ev, data_json, created_at
            FROM run_events WHERE id = ?
            "#,
        )
        .bind(&event_id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(event))
    }

    pub async fn mark_run_fix_outbox_broadcasted(&self, outbox_id: &str) -> StoreResult<bool> {
        let updated = sqlx::query(
            r#"
            UPDATE run_fix_event_outbox
            SET state = 'broadcasted', updated_at = current_timestamp
            WHERE id = ? AND state = 'event_persisted'
            "#,
        )
        .bind(outbox_id)
        .execute(self.pool())
        .await?;
        Ok(updated.rows_affected() == 1)
    }
}

pub(super) async fn finish_continuation_exhausted(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    source_run_id: &str,
    chain: &RunRepairChainRecord,
    attempts: i64,
    max_attempts: u32,
    reason_code: &str,
) -> StoreResult<()> {
    sqlx::query(
        r#"
        UPDATE run_failure_continuations
        SET state = 'fix_completed', reason_code = ?, updated_at = current_timestamp
        WHERE run_id = ? AND state IN ('exhausted', 'fix_pending', 'fix_claimed')
        "#,
    )
    .bind(reason_code)
    .bind(source_run_id)
    .execute(&mut **tx)
    .await?;
    insert_fix_outbox(
        tx,
        &format!("{}:exhausted", chain.id),
        source_run_id,
        "run.fix_exhausted",
        &serde_json::json!({
            "attempts": attempts,
            "max_attempts": max_attempts,
            "reason_code": reason_code,
        })
        .to_string(),
    )
    .await
}

async fn insert_fix_outbox(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    dedupe_key: &str,
    run_id: &str,
    event_name: &str,
    data_json: &str,
) -> StoreResult<()> {
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO run_fix_event_outbox (
            id, dedupe_key, run_id, event_name, data_json, state, created_at, updated_at
        )
        VALUES (?, ?, ?, ?, ?, 'pending', current_timestamp, current_timestamp)
        "#,
    )
    .bind(new_id("fixevt"))
    .bind(dedupe_key)
    .bind(run_id)
    .bind(event_name)
    .bind(data_json)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(super) async fn repair_chain_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    chain_id: &str,
) -> StoreResult<RunRepairChainRecord> {
    Ok(sqlx::query_as::<_, RunRepairChainRecord>(
        r#"
        SELECT id, workspace_id, root_run_id, provenance, sweep_group_id,
               recommended_run_id, created_at
        FROM run_repair_chains WHERE id = ?
        "#,
    )
    .bind(chain_id)
    .fetch_one(&mut **tx)
    .await?)
}

pub(super) async fn fix_attempt_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    attempt_id: &str,
) -> StoreResult<RunFixAttemptRecord> {
    Ok(
        sqlx::query_as::<_, RunFixAttemptRecord>(fix_attempt_sql!(" WHERE id = ?"))
            .bind(attempt_id)
            .fetch_one(&mut **tx)
            .await?,
    )
}

pub(super) async fn run_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    run_id: &str,
) -> StoreResult<RunRecord> {
    Ok(sqlx::query_as::<_, RunRecord>(
        r#"
        SELECT id, workspace_id, version_id, group_id, label, trigger, plan_json,
               estimate_json, status, error_json, started_at, ended_at, created_at,
               parent_run_id, attempt, force_rerun
        FROM runs WHERE id = ?
        "#,
    )
    .bind(run_id)
    .fetch_one(&mut **tx)
    .await?)
}

pub(super) fn require_one_write(operation: &'static str, rows: u64) -> StoreResult<()> {
    if rows == 1 {
        Ok(())
    } else {
        Err(StoreError::StatementInvariant {
            operation,
            expected_rows: 1,
            actual_rows: rows,
        })
    }
}
