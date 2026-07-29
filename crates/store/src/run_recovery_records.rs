use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Row};

use super::{RunRecord, Store, StoreError, StoreResult, new_id};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, FromRow)]
pub struct RunFailureContinuationRecord {
    pub run_id: String,
    pub state: String,
    pub retry_key: Option<String>,
    pub child_run_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Store {
    pub async fn claim_run_terminalization(
        &self,
        run_id: &str,
        owner_id: &str,
        lease_seconds: i64,
    ) -> StoreResult<bool> {
        if lease_seconds <= 0 {
            return Err(StoreError::RecoveryInvariant {
                operation: "claim_run_terminalization",
                message: "lease must be positive".to_owned(),
            });
        }
        let lease = format!("+{lease_seconds} seconds");
        let result = sqlx::query(
            r#"
            UPDATE run_terminalization_work_items
            SET owner_id = ?,
                lease_expires_at = datetime('now', ?),
                updated_at = current_timestamp
            WHERE run_id = ? AND state = 'settling'
              AND (
                owner_id IS NULL
                OR owner_id = ?
                OR lease_expires_at <= current_timestamp
              )
            "#,
        )
        .bind(owner_id)
        .bind(lease)
        .bind(run_id)
        .bind(owner_id)
        .execute(self.pool())
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn complete_run_terminalization(
        &self,
        run_id: &str,
        owner_id: &str,
    ) -> StoreResult<Option<RunRecord>> {
        let mut tx = self.pool().begin().await?;
        let work = sqlx::query(
            r#"
            SELECT desired_status, error_json
            FROM run_terminalization_work_items
            WHERE run_id = ? AND state = 'settling' AND owner_id = ?
              AND lease_expires_at > current_timestamp
            "#,
        )
        .bind(run_id)
        .bind(owner_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(work) = work else {
            tx.rollback().await?;
            return Ok(None);
        };
        let remaining: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM run_provider_tasks
            WHERE run_id = ? AND state IN ('dispatching', 'active', 'result_ready')
            "#,
        )
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?;
        if remaining != 0 {
            tx.rollback().await?;
            return Ok(None);
        }
        let desired_status: String = sqlx::Row::try_get(&work, "desired_status")?;
        let error_json: Option<String> = sqlx::Row::try_get(&work, "error_json")?;
        sqlx::query(
            r#"
            UPDATE run_steps
            SET state = 'skipped',
                ended_at = current_timestamp
            WHERE run_id = ? AND state IN ('queued', 'running')
            "#,
        )
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        let run_update = sqlx::query(
            r#"
            UPDATE runs
            SET status = ?,
                error_json = ?,
                ended_at = current_timestamp
            WHERE id = ? AND status IN ('queued', 'estimating', 'running')
            "#,
        )
        .bind(&desired_status)
        .bind(error_json.as_deref())
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        if run_update.rows_affected() != 1 {
            tx.rollback().await?;
            return Ok(None);
        }
        if desired_status == "failed" {
            sqlx::query(
                r#"
                INSERT OR IGNORE INTO run_failure_continuations (
                    run_id, state, retry_key, created_at, updated_at
                )
                VALUES (?, 'pending', ?, current_timestamp, current_timestamp)
                "#,
            )
            .bind(run_id)
            .bind(format!("retry:{run_id}"))
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(
            r#"
            UPDATE run_terminalization_work_items
            SET state = 'completed',
                completed_at = current_timestamp,
                updated_at = current_timestamp
            WHERE run_id = ? AND state = 'settling' AND owner_id = ?
            "#,
        )
        .bind(run_id)
        .bind(owner_id)
        .execute(&mut *tx)
        .await?;
        let run = sqlx::query_as::<_, RunRecord>(
            r#"
            SELECT id, workspace_id, version_id, group_id, label, trigger, plan_json,
                   estimate_json, status, error_json, started_at, ended_at, created_at,
                   parent_run_id, attempt, force_rerun
            FROM runs
            WHERE id = ?
            "#,
        )
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(run))
    }

    pub async fn failure_continuation(
        &self,
        run_id: &str,
    ) -> StoreResult<Option<RunFailureContinuationRecord>> {
        Ok(sqlx::query_as::<_, RunFailureContinuationRecord>(
            r#"
            SELECT run_id, state, retry_key, child_run_id, created_at, updated_at
            FROM run_failure_continuations
            WHERE run_id = ?
            "#,
        )
        .bind(run_id)
        .fetch_optional(self.pool())
        .await?)
    }

    pub async fn pending_failure_continuations(
        &self,
    ) -> StoreResult<Vec<RunFailureContinuationRecord>> {
        Ok(sqlx::query_as::<_, RunFailureContinuationRecord>(
            r#"
            SELECT run_id, state, retry_key, child_run_id, created_at, updated_at
            FROM run_failure_continuations
            WHERE state IN ('pending', 'retry_created')
            ORDER BY created_at, run_id
            "#,
        )
        .fetch_all(self.pool())
        .await?)
    }

    pub async fn mark_failure_retry_created(
        &self,
        run_id: &str,
        child_run_id: &str,
    ) -> StoreResult<Option<RunFailureContinuationRecord>> {
        let result = sqlx::query(
            r#"
            UPDATE run_failure_continuations
            SET state = 'retry_created',
                child_run_id = ?,
                updated_at = current_timestamp
            WHERE run_id = ? AND state = 'pending'
            "#,
        )
        .bind(child_run_id)
        .bind(run_id)
        .execute(self.pool())
        .await?;
        if result.rows_affected() == 0 {
            return self.failure_continuation(run_id).await;
        }
        self.failure_continuation(run_id).await
    }

    pub async fn complete_failure_continuation(
        &self,
        run_id: &str,
        exhausted: bool,
    ) -> StoreResult<Option<RunFailureContinuationRecord>> {
        let state = if exhausted { "exhausted" } else { "completed" };
        let result = sqlx::query(
            r#"
            UPDATE run_failure_continuations
            SET state = ?,
                updated_at = current_timestamp
            WHERE run_id = ? AND state IN ('pending', 'retry_created')
            "#,
        )
        .bind(state)
        .bind(run_id)
        .execute(self.pool())
        .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.failure_continuation(run_id).await
    }

    pub async fn create_retry_run_once(
        &self,
        parent_run_id: &str,
        force_rerun: bool,
    ) -> StoreResult<RunRecord> {
        let mut tx = self.pool().begin().await?;
        let continuation = sqlx::query_as::<_, RunFailureContinuationRecord>(
            r#"
            SELECT run_id, state, retry_key, child_run_id, created_at, updated_at
            FROM run_failure_continuations
            WHERE run_id = ?
            "#,
        )
        .bind(parent_run_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(continuation) = continuation else {
            return Err(StoreError::RecoveryInvariant {
                operation: "create_retry_run_once",
                message: format!("run `{parent_run_id}` has no durable continuation"),
            });
        };
        if let Some(child_run_id) = continuation.child_run_id {
            let child = run_in_transaction(&mut tx, &child_run_id).await?;
            tx.commit().await?;
            return Ok(child);
        }
        if continuation.state != "pending" {
            return Err(StoreError::RecoveryInvariant {
                operation: "create_retry_run_once",
                message: format!(
                    "run `{parent_run_id}` continuation is `{}` without a child",
                    continuation.state
                ),
            });
        }
        let parent = run_in_transaction(&mut tx, parent_run_id).await?;
        let child_id = new_id("run");
        let trigger = if parent.trigger == "sweep" {
            "agent"
        } else {
            &parent.trigger
        };
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO runs (
                id, workspace_id, version_id, group_id, label, trigger, plan_json,
                estimate_json, status, parent_run_id, attempt, force_rerun, created_at
            )
            VALUES (?, ?, ?, NULL, ?, ?, ?, ?, 'waiting_confirmation', ?, ?, ?,
                    current_timestamp)
            "#,
        )
        .bind(&child_id)
        .bind(&parent.workspace_id)
        .bind(&parent.version_id)
        .bind(&parent.label)
        .bind(trigger)
        .bind(parent.plan_json.as_deref())
        .bind(parent.estimate_json.as_deref())
        .bind(&parent.id)
        .bind(parent.attempt + 1)
        .bind(if force_rerun { 1_i64 } else { 0_i64 })
        .execute(&mut *tx)
        .await?;
        let estimates = sqlx::query(
            r#"
            SELECT id, workspace_id, provider, amount, currency
            FROM cost_ledger
            WHERE run_id = ? AND estimated = 1
            ORDER BY created_at, id
            "#,
        )
        .bind(&parent.id)
        .fetch_all(&mut *tx)
        .await?;
        for estimate in estimates {
            let source_cost_id: String = estimate.try_get("id")?;
            sqlx::query(
                r#"
                INSERT OR IGNORE INTO cost_ledger (
                    id, workspace_id, run_id, run_step_id, provider, amount,
                    currency, estimated, operation_key, created_at
                )
                VALUES (?, ?, ?, NULL, ?, ?, ?, 1, ?, current_timestamp)
                "#,
            )
            .bind(new_id("cost"))
            .bind(estimate.try_get::<String, _>("workspace_id")?)
            .bind(&child_id)
            .bind(estimate.try_get::<String, _>("provider")?)
            .bind(estimate.try_get::<f64, _>("amount")?)
            .bind(estimate.try_get::<String, _>("currency")?)
            .bind(format!("retry-estimate:{child_id}:{source_cost_id}"))
            .execute(&mut *tx)
            .await?;
        }
        let linked = sqlx::query(
            r#"
            UPDATE run_failure_continuations
            SET state = 'retry_created',
                child_run_id = ?,
                updated_at = current_timestamp
            WHERE run_id = ? AND state = 'pending' AND child_run_id IS NULL
            "#,
        )
        .bind(&child_id)
        .bind(parent_run_id)
        .execute(&mut *tx)
        .await?;
        if linked.rows_affected() != 1 {
            return Err(StoreError::RecoveryInvariant {
                operation: "create_retry_run_once",
                message: format!("run `{parent_run_id}` retry linkage lost"),
            });
        }
        let child = run_in_transaction(&mut tx, &child_id).await?;
        tx.commit().await?;
        Ok(child)
    }
}

async fn run_in_transaction(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    run_id: &str,
) -> StoreResult<RunRecord> {
    Ok(sqlx::query_as::<_, RunRecord>(
        r#"
        SELECT id, workspace_id, version_id, group_id, label, trigger, plan_json,
               estimate_json, status, error_json, started_at, ended_at, created_at,
               parent_run_id, attempt, force_rerun
        FROM runs
        WHERE id = ?
        "#,
    )
    .bind(run_id)
    .fetch_one(&mut **tx)
    .await?)
}
