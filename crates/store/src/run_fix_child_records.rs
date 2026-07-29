use sqlx::Row;

use crate::run_fix_records::{
    finish_continuation_exhausted, fix_attempt_in_tx, repair_chain_in_tx, require_one_write,
    run_in_tx,
};

use super::{RunFixAttemptRecord, RunRecord, Store, StoreError, StoreResult, new_id};

#[derive(Debug, Clone)]
pub struct PrepareRunFixChildRecord<'a> {
    pub attempt_id: &'a str,
    pub label: &'a str,
    pub plan_json: &'a str,
    pub actual_runtime_provider_id: Option<&'a str>,
    pub actual_effective_provider_id: &'a str,
    pub actual_recovery_scope_fingerprint: &'a str,
    pub actual_provider_catalog_fingerprint: &'a str,
}

#[derive(Debug, Clone)]
pub struct CompleteRunFixChildRecord<'a> {
    pub attempt_id: &'a str,
    pub estimate_json: &'a str,
    pub requires_confirmation: bool,
}

impl Store {
    pub async fn cancel_run_fix_child(&self, child_run_id: &str) -> StoreResult<Option<RunRecord>> {
        let mut tx = self.pool().begin_with("BEGIN IMMEDIATE").await?;
        let attempt = sqlx::query_as::<_, RunFixAttemptRecord>(
            r#"
            SELECT id, operation_id, workspace_id, chain_id, root_run_id, source_run_id,
                   attempt_index, max_attempts, source_version_id,
                   expected_runtime_provider_id, effective_provider_id,
                   expected_recovery_scope_fingerprint, provider_catalog_fingerprint,
                   state, proposal_id, target_version_id, child_run_id, reason_code,
                   error_summary, created_at, updated_at, finished_at
            FROM run_fix_attempts WHERE child_run_id = ?
            "#,
        )
        .bind(child_run_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(attempt) = attempt else {
            tx.rollback().await?;
            return Ok(None);
        };
        if attempt.state == "cancelled" {
            let child = run_in_tx(&mut tx, child_run_id).await?;
            tx.commit().await?;
            return Ok(Some(child));
        }
        if !matches!(
            attempt.state.as_str(),
            "version_applied" | "child_preparing" | "child_ready"
        ) {
            tx.rollback().await?;
            return Ok(None);
        }
        sqlx::query(
            r#"
            UPDATE run_fix_attempts
            SET state = 'cancelled', reason_code = 'FIX_USER_CANCELLED',
                finished_at = current_timestamp, updated_at = current_timestamp
            WHERE id = ? AND state IN ('version_applied', 'child_preparing', 'child_ready')
            "#,
        )
        .bind(&attempt.id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE run_failure_continuations
            SET state = 'fix_completed', reason_code = 'FIX_USER_CANCELLED',
                updated_at = current_timestamp
            WHERE run_id = ?
            "#,
        )
        .bind(&attempt.source_run_id)
        .execute(&mut *tx)
        .await?;
        let remote_tasks: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM run_provider_tasks
            WHERE run_id = ? AND state IN ('dispatching', 'active', 'result_ready')
            "#,
        )
        .bind(child_run_id)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE run_steps SET state = 'skipped', ended_at = current_timestamp
            WHERE run_id = ? AND state = 'queued'
            "#,
        )
        .bind(child_run_id)
        .execute(&mut *tx)
        .await?;
        if remote_tasks == 0 {
            sqlx::query(
                r#"
                UPDATE runs SET status = 'interrupted', error_json = NULL,
                    ended_at = current_timestamp
                WHERE id = ? AND status IN (
                    'queued', 'estimating', 'waiting_confirmation', 'running'
                )
                "#,
            )
            .bind(child_run_id)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query(
                r#"
                INSERT INTO run_terminalization_work_items (
                    run_id, desired_status, state, error_json, created_at, updated_at
                )
                VALUES (?, 'interrupted', 'settling', NULL, current_timestamp, current_timestamp)
                ON CONFLICT(run_id) DO UPDATE SET
                    desired_status = 'interrupted', error_json = NULL,
                    updated_at = current_timestamp
                WHERE run_terminalization_work_items.state = 'settling'
                "#,
            )
            .bind(child_run_id)
            .execute(&mut *tx)
            .await?;
        }
        let child = run_in_tx(&mut tx, child_run_id).await?;
        tx.commit().await?;
        Ok(Some(child))
    }

    pub async fn exhaust_run_fix_without_attempt(
        &self,
        source_run_id: &str,
        max_attempts: u32,
        reason_code: &str,
    ) -> StoreResult<bool> {
        let mut tx = self.pool().begin_with("BEGIN IMMEDIATE").await?;
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
            return Ok(false);
        };
        let state: String = continuation.try_get("state")?;
        let chain_id: Option<String> = continuation.try_get("chain_id")?;
        if !matches!(state.as_str(), "exhausted" | "fix_pending") {
            tx.rollback().await?;
            return Ok(false);
        }
        let Some(chain_id) = chain_id else {
            tx.rollback().await?;
            return Ok(false);
        };
        let chain = repair_chain_in_tx(&mut tx, &chain_id).await?;
        let attempts: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM run_fix_attempts WHERE chain_id = ?")
                .bind(&chain_id)
                .fetch_one(&mut *tx)
                .await?;
        finish_continuation_exhausted(
            &mut tx,
            source_run_id,
            &chain,
            attempts,
            max_attempts,
            reason_code,
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn pending_run_fix_source_ids(&self) -> StoreResult<Vec<String>> {
        Ok(sqlx::query_scalar(
            r#"
            SELECT continuation.run_id
            FROM run_failure_continuations AS continuation
            JOIN runs AS source ON source.id = continuation.run_id
            JOIN run_repair_chains AS chain ON chain.id = continuation.chain_id
            WHERE continuation.state IN ('exhausted', 'fix_pending')
              AND source.status = 'failed'
            ORDER BY continuation.created_at, continuation.run_id
            "#,
        )
        .fetch_all(self.pool())
        .await?)
    }

    pub async fn unfinished_run_fix_attempts(&self) -> StoreResult<Vec<RunFixAttemptRecord>> {
        Ok(sqlx::query_as::<_, RunFixAttemptRecord>(
            r#"
            SELECT id, operation_id, workspace_id, chain_id, root_run_id, source_run_id,
                   attempt_index, max_attempts, source_version_id,
                   expected_runtime_provider_id, effective_provider_id,
                   expected_recovery_scope_fingerprint, provider_catalog_fingerprint,
                   state, proposal_id, target_version_id, child_run_id, reason_code,
                   error_summary, created_at, updated_at, finished_at
            FROM run_fix_attempts
            WHERE state IN ('claimed', 'agent_running', 'version_applied', 'child_preparing')
            ORDER BY created_at, id
            "#,
        )
        .fetch_all(self.pool())
        .await?)
    }

    pub async fn run_fix_attempt_for_child(
        &self,
        child_run_id: &str,
    ) -> StoreResult<Option<RunFixAttemptRecord>> {
        Ok(sqlx::query_as::<_, RunFixAttemptRecord>(
            r#"
            SELECT id, operation_id, workspace_id, chain_id, root_run_id, source_run_id,
                   attempt_index, max_attempts, source_version_id,
                   expected_runtime_provider_id, effective_provider_id,
                   expected_recovery_scope_fingerprint, provider_catalog_fingerprint,
                   state, proposal_id, target_version_id, child_run_id, reason_code,
                   error_summary, created_at, updated_at, finished_at
            FROM run_fix_attempts WHERE child_run_id = ?
            "#,
        )
        .bind(child_run_id)
        .fetch_optional(self.pool())
        .await?)
    }

    pub async fn prepare_run_fix_child(
        &self,
        input: PrepareRunFixChildRecord<'_>,
    ) -> StoreResult<(RunFixAttemptRecord, RunRecord)> {
        let mut tx = self.pool().begin_with("BEGIN IMMEDIATE").await?;
        let attempt = fix_attempt_in_tx(&mut tx, input.attempt_id).await?;
        if matches!(attempt.state.as_str(), "child_preparing" | "child_ready") {
            let child_id =
                attempt
                    .child_run_id
                    .as_deref()
                    .ok_or(StoreError::RunFixGuardConflict {
                        code: "FIX_RECOVERY_INTERRUPTED",
                    })?;
            let child = run_in_tx(&mut tx, child_id).await?;
            tx.commit().await?;
            return Ok((attempt, child));
        }
        validate_child_guard(&mut tx, &attempt, &input).await?;
        let target_version_id =
            attempt
                .target_version_id
                .as_deref()
                .ok_or(StoreError::RunFixGuardConflict {
                    code: "FIX_RECOVERY_INTERRUPTED",
                })?;
        let child_run_id = new_id("run");
        sqlx::query(
            r#"
            INSERT INTO runs (
                id, workspace_id, version_id, label, trigger, plan_json,
                status, force_rerun, created_at
            )
            VALUES (?, ?, ?, ?, 'agent', ?, 'estimating', 0, current_timestamp)
            "#,
        )
        .bind(&child_run_id)
        .bind(&attempt.workspace_id)
        .bind(target_version_id)
        .bind(input.label)
        .bind(input.plan_json)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO run_repair_chain_runs (run_id, chain_id, relation, created_at)
            VALUES (?, ?, 'fix_child', current_timestamp)
            "#,
        )
        .bind(&child_run_id)
        .bind(&attempt.chain_id)
        .execute(&mut *tx)
        .await?;
        let updated = sqlx::query(
            r#"
            UPDATE run_fix_attempts
            SET state = 'child_preparing', child_run_id = ?, updated_at = current_timestamp
            WHERE id = ? AND state = 'version_applied' AND child_run_id IS NULL
            "#,
        )
        .bind(&child_run_id)
        .bind(input.attempt_id)
        .execute(&mut *tx)
        .await?;
        require_one_write("link_run_fix_child", updated.rows_affected())?;
        let attempt = fix_attempt_in_tx(&mut tx, input.attempt_id).await?;
        let child = run_in_tx(&mut tx, &child_run_id).await?;
        tx.commit().await?;
        Ok((attempt, child))
    }

    pub async fn complete_run_fix_child(
        &self,
        input: CompleteRunFixChildRecord<'_>,
    ) -> StoreResult<(RunFixAttemptRecord, RunRecord)> {
        let mut tx = self.pool().begin_with("BEGIN IMMEDIATE").await?;
        let attempt = fix_attempt_in_tx(&mut tx, input.attempt_id).await?;
        let child_id = attempt
            .child_run_id
            .as_deref()
            .ok_or(StoreError::RunFixGuardConflict {
                code: "FIX_RECOVERY_INTERRUPTED",
            })?;
        if attempt.state == "child_ready" {
            let child = run_in_tx(&mut tx, child_id).await?;
            tx.commit().await?;
            return Ok((attempt, child));
        }
        if attempt.state != "child_preparing" {
            return Err(StoreError::RunFixGuardConflict {
                code: "FIX_RECOVERY_INTERRUPTED",
            });
        }
        let child_update = sqlx::query(
            r#"
            UPDATE runs SET estimate_json = ?, status = 'waiting_confirmation'
            WHERE id = ? AND status = 'estimating'
            "#,
        )
        .bind(input.estimate_json)
        .bind(child_id)
        .execute(&mut *tx)
        .await?;
        require_one_write("complete_run_fix_child", child_update.rows_affected())?;
        let attempt_update = sqlx::query(
            r#"
            UPDATE run_fix_attempts
            SET state = 'child_ready', updated_at = current_timestamp
            WHERE id = ? AND state = 'child_preparing'
            "#,
        )
        .bind(input.attempt_id)
        .execute(&mut *tx)
        .await?;
        require_one_write("complete_run_fix_attempt", attempt_update.rows_affected())?;
        sqlx::query(
            r#"
            UPDATE run_failure_continuations
            SET state = 'fix_completed', reason_code = 'FIX_APPLIED',
                updated_at = current_timestamp
            WHERE run_id = ? AND state = 'fix_claimed'
            "#,
        )
        .bind(&attempt.source_run_id)
        .execute(&mut *tx)
        .await?;
        let target_version_id =
            attempt
                .target_version_id
                .as_deref()
                .ok_or(StoreError::RunFixGuardConflict {
                    code: "FIX_RECOVERY_INTERRUPTED",
                })?;
        let data = serde_json::json!({
            "operation_id": attempt.operation_id,
            "attempt": attempt.attempt_index,
            "max_attempts": attempt.max_attempts,
            "source_version_id": attempt.source_version_id,
            "target_version_id": target_version_id,
            "child_run_id": child_id,
            "requires_confirmation": input.requires_confirmation,
        })
        .to_string();
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO run_fix_event_outbox (
                id, dedupe_key, run_id, event_name, data_json, state, created_at, updated_at
            )
            VALUES (?, ?, ?, 'run.fix_applied', ?, 'pending', current_timestamp, current_timestamp)
            "#,
        )
        .bind(new_id("fixevt"))
        .bind(format!("{}:applied", attempt.id))
        .bind(&attempt.source_run_id)
        .bind(data)
        .execute(&mut *tx)
        .await?;
        let attempt = fix_attempt_in_tx(&mut tx, input.attempt_id).await?;
        let child = run_in_tx(&mut tx, child_id).await?;
        tx.commit().await?;
        Ok((attempt, child))
    }
}

async fn validate_child_guard(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    attempt: &RunFixAttemptRecord,
    input: &PrepareRunFixChildRecord<'_>,
) -> StoreResult<()> {
    if attempt.state != "version_applied"
        || input.actual_effective_provider_id != attempt.effective_provider_id
    {
        return Err(StoreError::RunFixGuardConflict {
            code: "FIX_PROVIDER_CHANGED",
        });
    }
    if input.actual_recovery_scope_fingerprint != attempt.expected_recovery_scope_fingerprint {
        return Err(StoreError::RunFixGuardConflict {
            code: "FIX_PROVIDER_SCOPE_CHANGED",
        });
    }
    if input.actual_provider_catalog_fingerprint != attempt.provider_catalog_fingerprint {
        return Err(StoreError::RunFixGuardConflict {
            code: "FIX_PROVIDER_CHANGED",
        });
    }
    let workspace =
        sqlx::query("SELECT cur_version_id, runtime_provider_id FROM workspaces WHERE id = ?")
            .bind(&attempt.workspace_id)
            .fetch_one(&mut **tx)
            .await?;
    let current: Option<String> = workspace.try_get("cur_version_id")?;
    let selected: Option<String> = workspace.try_get("runtime_provider_id")?;
    if current != attempt.target_version_id {
        return Err(StoreError::RunFixGuardConflict {
            code: "FIX_TARGET_DESELECTED",
        });
    }
    if selected.as_deref() != input.actual_runtime_provider_id
        || selected != attempt.expected_runtime_provider_id
    {
        return Err(StoreError::RunFixGuardConflict {
            code: "FIX_PROVIDER_CHANGED",
        });
    }
    Ok(())
}
