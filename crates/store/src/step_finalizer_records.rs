use sqlx::Row;

use super::{Store, StoreError, StoreResult};

impl Store {
    pub async fn finalize_run_step_success(
        &self,
        run_step_id: &str,
        provider_task_id: Option<&str>,
        cost_actual_json: Option<&str>,
        outputs: &[(String, String)],
    ) -> StoreResult<bool> {
        let mut tx = self.pool().begin().await?;
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
