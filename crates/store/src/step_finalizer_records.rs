use sqlx::Row;

use super::{Store, StoreError, StoreResult};

#[derive(Debug, Clone)]
pub struct ProviderTaskFailureFinalization<'a> {
    pub provider_task_id: &'a str,
    pub expected_task_state: &'a str,
    pub terminal_task_state: &'a str,
    pub error_code: &'a str,
    pub desired_run_status: &'a str,
    pub error_json: Option<&'a str>,
    pub required_expired_foreign_owner: Option<&'a str>,
}

impl Store {
    pub async fn finalize_provider_task_failure(
        &self,
        input: ProviderTaskFailureFinalization<'_>,
    ) -> StoreResult<bool> {
        if !matches!(
            input.terminal_task_state,
            "completed" | "cancelled" | "abandoned"
        ) || !matches!(input.desired_run_status, "failed" | "interrupted")
        {
            return Err(StoreError::RecoveryInvariant {
                operation: "finalize_provider_task_failure",
                message: "unsupported provider or run terminal state".to_owned(),
            });
        }
        let mut tx = self.pool().begin().await?;
        sqlx::query("UPDATE run_provider_tasks SET updated_at = updated_at WHERE id = ?")
            .bind(input.provider_task_id)
            .execute(&mut *tx)
            .await?;
        let task = sqlx::query(
            "SELECT run_id, run_step_id, state, last_error_code, \
                    CASE WHEN ? IS NULL THEN 1 \
                         WHEN dispatch_owner_id <> ? \
                          AND dispatch_lease_expires_at <= current_timestamp THEN 1 \
                         ELSE 0 END AS owner_guard_matches \
             FROM run_provider_tasks WHERE id = ?",
        )
        .bind(input.required_expired_foreign_owner)
        .bind(input.required_expired_foreign_owner)
        .bind(input.provider_task_id)
        .fetch_one(&mut *tx)
        .await?;
        let run_id: String = task.try_get("run_id")?;
        let run_step_id: String = task.try_get("run_step_id")?;
        let current_state: String = task.try_get("state")?;
        let current_error_code: Option<String> = task.try_get("last_error_code")?;
        let owner_guard_matches: bool = task.try_get("owner_guard_matches")?;
        if !owner_guard_matches {
            tx.rollback().await?;
            return Ok(false);
        }
        if current_state != input.terminal_task_state {
            if current_state != input.expected_task_state {
                tx.rollback().await?;
                return Ok(false);
            }
            let updated = sqlx::query(
                r#"
                UPDATE run_provider_tasks
                SET state = ?,
                    last_error_code = ?,
                    dispatch_owner_id = NULL,
                    dispatch_lease_expires_at = NULL,
                    ended_at = current_timestamp,
                    updated_at = current_timestamp
                WHERE id = ? AND state = ?
                "#,
            )
            .bind(input.terminal_task_state)
            .bind(input.error_code)
            .bind(input.provider_task_id)
            .bind(input.expected_task_state)
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() != 1 {
                tx.rollback().await?;
                return Ok(false);
            }
        } else if current_error_code.as_deref() != Some(input.error_code) {
            tx.rollback().await?;
            return Ok(false);
        }
        let step_updated = sqlx::query(
            r#"
            UPDATE run_steps
            SET state = 'failed',
                progress = 1.0,
                error_json = ?,
                started_at = COALESCE(started_at, current_timestamp),
                ended_at = current_timestamp
            WHERE id = ? AND state IN ('queued', 'running', 'failed')
            "#,
        )
        .bind(input.error_json)
        .bind(&run_step_id)
        .execute(&mut *tx)
        .await?;
        if step_updated.rows_affected() != 1 {
            tx.rollback().await?;
            return Ok(false);
        }
        sqlx::query(
            r#"
            INSERT INTO run_terminalization_work_items (
                run_id, desired_status, state, error_json, created_at, updated_at
            )
            VALUES (?, ?, 'settling', ?, current_timestamp, current_timestamp)
            ON CONFLICT(run_id) DO UPDATE SET
                desired_status = CASE
                    WHEN excluded.desired_status = 'interrupted'
                    THEN 'interrupted'
                    ELSE run_terminalization_work_items.desired_status
                END,
                error_json = CASE
                    WHEN excluded.desired_status = 'interrupted'
                      OR run_terminalization_work_items.desired_status = 'interrupted'
                    THEN NULL
                    ELSE COALESCE(run_terminalization_work_items.error_json, excluded.error_json)
                END,
                updated_at = current_timestamp
            WHERE run_terminalization_work_items.state = 'settling'
            "#,
        )
        .bind(&run_id)
        .bind(input.desired_run_status)
        .bind(input.error_json)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn finalize_run_step_success(
        &self,
        run_step_id: &str,
        provider_task_id: Option<&str>,
        cost_actual_json: Option<&str>,
        outputs: &[(String, String)],
    ) -> StoreResult<bool> {
        let mut tx = self.pool().begin().await?;
        sqlx::query("UPDATE run_steps SET progress = progress WHERE id = ?")
            .bind(run_step_id)
            .execute(&mut *tx)
            .await?;
        if let Some(task_id) = provider_task_id {
            let state: String =
                sqlx::query_scalar("SELECT state FROM run_provider_tasks WHERE id = ?")
                    .bind(task_id)
                    .fetch_one(&mut *tx)
                    .await?;
            if !matches!(state.as_str(), "result_ready" | "completed") {
                tx.rollback().await?;
                return Ok(false);
            }
        }
        for (port, artifact_id) in outputs {
            sqlx::query(
                r#"
                INSERT OR IGNORE INTO run_step_outputs (
                    run_step_id, port, artifact_id, created_at
                )
                VALUES (?, ?, ?, current_timestamp)
                "#,
            )
            .bind(run_step_id)
            .bind(port)
            .bind(artifact_id)
            .execute(&mut *tx)
            .await?;
            let existing: String = sqlx::query(
                "SELECT artifact_id FROM run_step_outputs WHERE run_step_id = ? AND port = ?",
            )
            .bind(run_step_id)
            .bind(port)
            .fetch_one(&mut *tx)
            .await?
            .try_get("artifact_id")?;
            if existing != *artifact_id {
                return Err(StoreError::RecoveryInvariant {
                    operation: "finalize_run_step_success",
                    message: format!(
                        "step `{run_step_id}` port `{port}` already maps to another artifact"
                    ),
                });
            }
        }
        let step = sqlx::query(
            r#"
            UPDATE run_steps
            SET state = 'succeeded',
                progress = 1.0,
                cost_actual_json = ?,
                error_json = NULL,
                started_at = COALESCE(started_at, current_timestamp),
                ended_at = current_timestamp
            WHERE id = ? AND state IN ('queued', 'running', 'succeeded')
            "#,
        )
        .bind(cost_actual_json)
        .bind(run_step_id)
        .execute(&mut *tx)
        .await?;
        if step.rows_affected() != 1 {
            tx.rollback().await?;
            return Ok(false);
        }
        if let Some(task_id) = provider_task_id {
            sqlx::query(
                r#"
                UPDATE run_provider_tasks
                SET state = 'completed',
                    dispatch_owner_id = NULL,
                    dispatch_lease_expires_at = NULL,
                    ended_at = COALESCE(ended_at, current_timestamp),
                    updated_at = current_timestamp
                WHERE id = ? AND state = 'result_ready'
                "#,
            )
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(true)
    }
}
