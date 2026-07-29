use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Row, Sqlite, Transaction};

use super::{Store, StoreError, StoreResult, new_id};

pub const PROVIDER_TASK_DISPATCHING: &str = "dispatching";
pub const PROVIDER_TASK_ACTIVE: &str = "active";
pub const PROVIDER_TASK_RESULT_READY: &str = "result_ready";
pub const PROVIDER_TASK_COMPLETED: &str = "completed";
pub const PROVIDER_TASK_CANCELLED: &str = "cancelled";
pub const PROVIDER_TASK_ABANDONED: &str = "abandoned";

macro_rules! provider_task_sql {
    ($suffix:literal) => {
        concat!(
            r#"
            SELECT id, run_id, run_step_id, provider, dispatch_origin,
                   recovery_scope_fingerprint, operation_key, state,
                   dispatch_owner_id, dispatch_lease_expires_at, dispatch_deadline_at,
                   provider_task_id, status_url, result_url, terminal_outcome,
                   result_spool_path, result_fingerprint, materialization_deadline_at,
                   materialization_attempts, materialization_next_retry_at,
                   last_error_code, recovery_deadline_at, created_at, updated_at, ended_at
            FROM run_provider_tasks
            "#,
            $suffix
        )
    };
}

#[derive(Debug, Clone)]
pub struct NewProviderTask<'a> {
    pub run_id: &'a str,
    pub run_step_id: &'a str,
    pub provider: &'a str,
    pub dispatch_origin: &'a str,
    pub recovery_scope_fingerprint: &'a str,
    pub operation_key: &'a str,
    pub dispatch_owner_id: &'a str,
    pub dispatch_lease_seconds: i64,
    pub dispatch_deadline_seconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, FromRow)]
pub struct ProviderTaskRecord {
    pub id: String,
    pub run_id: String,
    pub run_step_id: String,
    pub provider: String,
    pub dispatch_origin: String,
    pub recovery_scope_fingerprint: String,
    pub operation_key: String,
    pub state: String,
    pub dispatch_owner_id: Option<String>,
    pub dispatch_lease_expires_at: Option<String>,
    pub dispatch_deadline_at: Option<String>,
    pub provider_task_id: Option<String>,
    pub status_url: Option<String>,
    pub result_url: Option<String>,
    pub terminal_outcome: Option<String>,
    pub result_spool_path: Option<String>,
    pub result_fingerprint: Option<String>,
    pub materialization_deadline_at: Option<String>,
    pub materialization_attempts: i64,
    pub materialization_next_retry_at: Option<String>,
    pub last_error_code: Option<String>,
    pub recovery_deadline_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProviderTaskHandleUpdate<'a> {
    pub task_id: &'a str,
    pub dispatch_owner_id: &'a str,
    pub provider_task_id: &'a str,
    pub status_url: Option<&'a str>,
    pub result_url: Option<&'a str>,
    pub recovery_deadline_seconds: i64,
}

#[derive(Debug, Clone)]
pub struct ProviderTaskResult<'a> {
    pub task_id: &'a str,
    pub dispatch_owner_id: Option<&'a str>,
    pub terminal_outcome: &'a str,
    pub result_spool_path: &'a str,
    pub result_fingerprint: &'a str,
    pub materialization_deadline_seconds: i64,
    pub workspace_id: &'a str,
    pub provider: &'a str,
    pub amount: f64,
    pub currency: &'a str,
    pub estimated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, FromRow)]
pub struct RunStepOutputRecord {
    pub run_step_id: String,
    pub port: String,
    pub artifact_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, FromRow)]
pub struct RunRecoveryLeaseRecord {
    pub run_id: String,
    pub owner_id: String,
    pub lease_expires_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, FromRow)]
pub struct RunExecutionIntentRecord {
    pub run_id: String,
    pub plan_fingerprint: String,
    pub estimate_fingerprint: String,
    pub cost_decision: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, FromRow)]
pub struct RunTerminalizationRecord {
    pub run_id: String,
    pub desired_status: String,
    pub state: String,
    pub error_json: Option<String>,
    pub owner_id: Option<String>,
    pub lease_expires_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRow)]
pub struct ProviderTaskTiming {
    pub dispatch_owner_live: bool,
    pub recovery_expired: bool,
    pub materialization_expired: bool,
    pub materialization_due: bool,
}

impl Store {
    pub async fn insert_or_read_provider_task(
        &self,
        input: NewProviderTask<'_>,
    ) -> StoreResult<ProviderTaskRecord> {
        let id = new_id("ptask");
        let lease = positive_duration(input.dispatch_lease_seconds, "dispatch lease")?;
        let deadline = positive_duration(input.dispatch_deadline_seconds, "dispatch deadline")?;
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO run_provider_tasks (
                id, run_id, run_step_id, provider, dispatch_origin,
                recovery_scope_fingerprint, operation_key, state,
                dispatch_owner_id, dispatch_lease_expires_at, dispatch_deadline_at,
                created_at, updated_at
            )
            VALUES (
                ?, ?, ?, ?, ?, ?, ?, 'dispatching', ?,
                datetime('now', ?), datetime('now', ?),
                current_timestamp, current_timestamp
            )
            "#,
        )
        .bind(id)
        .bind(input.run_id)
        .bind(input.run_step_id)
        .bind(input.provider)
        .bind(input.dispatch_origin)
        .bind(input.recovery_scope_fingerprint)
        .bind(input.operation_key)
        .bind(input.dispatch_owner_id)
        .bind(lease)
        .bind(deadline)
        .execute(self.pool())
        .await?;
        let task = self
            .provider_task_for_step(input.run_step_id)
            .await?
            .ok_or_else(|| StoreError::RecoveryInvariant {
                operation: "insert_or_read_provider_task",
                message: format!(
                    "step `{}` provider task disappeared after insert",
                    input.run_step_id
                ),
            })?;
        if task.run_id != input.run_id
            || task.provider != input.provider
            || task.dispatch_origin != input.dispatch_origin
            || task.recovery_scope_fingerprint != input.recovery_scope_fingerprint
            || task.operation_key != input.operation_key
        {
            return Err(StoreError::RecoveryInvariant {
                operation: "insert_or_read_provider_task",
                message: format!(
                    "step `{}` already has a provider task with different immutable identity",
                    input.run_step_id
                ),
            });
        }
        Ok(task)
    }

    pub async fn provider_task(&self, task_id: &str) -> StoreResult<ProviderTaskRecord> {
        Ok(
            sqlx::query_as::<_, ProviderTaskRecord>(provider_task_sql!("WHERE id = ?"))
                .bind(task_id)
                .fetch_one(self.pool())
                .await?,
        )
    }

    pub async fn provider_task_for_step(
        &self,
        run_step_id: &str,
    ) -> StoreResult<Option<ProviderTaskRecord>> {
        Ok(
            sqlx::query_as::<_, ProviderTaskRecord>(provider_task_sql!("WHERE run_step_id = ?"))
                .bind(run_step_id)
                .fetch_optional(self.pool())
                .await?,
        )
    }

    pub async fn provider_tasks_for_run(
        &self,
        run_id: &str,
    ) -> StoreResult<Vec<ProviderTaskRecord>> {
        Ok(sqlx::query_as::<_, ProviderTaskRecord>(provider_task_sql!(
            "WHERE run_id = ? ORDER BY created_at, id"
        ))
        .bind(run_id)
        .fetch_all(self.pool())
        .await?)
    }

    pub async fn active_provider_tasks(&self) -> StoreResult<Vec<ProviderTaskRecord>> {
        Ok(sqlx::query_as::<_, ProviderTaskRecord>(provider_task_sql!(
            "WHERE state IN ('dispatching', 'active', 'result_ready') ORDER BY created_at, id"
        ))
        .fetch_all(self.pool())
        .await?)
    }

    pub async fn provider_task_timing(&self, task_id: &str) -> StoreResult<ProviderTaskTiming> {
        Ok(sqlx::query_as::<_, ProviderTaskTiming>(
            r#"
            SELECT
              state = 'dispatching'
                AND dispatch_lease_expires_at > current_timestamp AS dispatch_owner_live,
              state = 'active'
                AND recovery_deadline_at <= current_timestamp AS recovery_expired,
              state = 'result_ready'
                AND materialization_deadline_at <= current_timestamp AS materialization_expired,
              state = 'result_ready'
                AND materialization_next_retry_at <= current_timestamp AS materialization_due
            FROM run_provider_tasks
            WHERE id = ?
            "#,
        )
        .bind(task_id)
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn renew_dispatch_owner(
        &self,
        task_id: &str,
        owner_id: &str,
        lease_seconds: i64,
    ) -> StoreResult<bool> {
        let lease = positive_duration(lease_seconds, "dispatch lease")?;
        let result = sqlx::query(
            r#"
            UPDATE run_provider_tasks
            SET dispatch_lease_expires_at = datetime('now', ?),
                updated_at = current_timestamp
            WHERE id = ? AND state = 'dispatching' AND dispatch_owner_id = ?
              AND dispatch_deadline_at > current_timestamp
            "#,
        )
        .bind(lease)
        .bind(task_id)
        .bind(owner_id)
        .execute(self.pool())
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn activate_provider_task(
        &self,
        input: ProviderTaskHandleUpdate<'_>,
    ) -> StoreResult<Option<ProviderTaskRecord>> {
        let recovery_deadline =
            positive_duration(input.recovery_deadline_seconds, "recovery deadline")?;
        let result = sqlx::query(
            r#"
            UPDATE run_provider_tasks
            SET state = 'active',
                provider_task_id = ?,
                status_url = ?,
                result_url = ?,
                recovery_deadline_at = datetime('now', ?),
                dispatch_lease_expires_at = NULL,
                updated_at = current_timestamp
            WHERE id = ? AND state = 'dispatching' AND dispatch_owner_id = ?
            "#,
        )
        .bind(input.provider_task_id)
        .bind(input.status_url)
        .bind(input.result_url)
        .bind(recovery_deadline)
        .bind(input.task_id)
        .bind(input.dispatch_owner_id)
        .execute(self.pool())
        .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.provider_task(input.task_id).await.map(Some)
    }

    pub async fn mark_provider_task_result_ready(
        &self,
        input: ProviderTaskResult<'_>,
    ) -> StoreResult<Option<ProviderTaskRecord>> {
        if !input.amount.is_finite() || input.amount < 0.0 {
            return Err(StoreError::RecoveryInvariant {
                operation: "mark_provider_task_result_ready",
                message: "actual cost must be finite and non-negative".to_owned(),
            });
        }
        let deadline = positive_duration(
            input.materialization_deadline_seconds,
            "materialization deadline",
        )?;
        let mut tx = self.pool().begin().await?;
        let result = sqlx::query(
            r#"
            UPDATE run_provider_tasks
            SET state = 'result_ready',
                terminal_outcome = ?,
                result_spool_path = ?,
                result_fingerprint = ?,
                materialization_deadline_at = datetime('now', ?),
                materialization_attempts = 0,
                materialization_next_retry_at = current_timestamp,
                dispatch_lease_expires_at = NULL,
                updated_at = current_timestamp
            WHERE id = ?
              AND (
                state = 'active'
                OR (
                  state = 'dispatching'
                  AND dispatch_owner_id = ?
                )
              )
            "#,
        )
        .bind(input.terminal_outcome)
        .bind(input.result_spool_path)
        .bind(input.result_fingerprint)
        .bind(deadline)
        .bind(input.task_id)
        .bind(input.dispatch_owner_id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() != 1 {
            tx.rollback().await?;
            return Ok(None);
        }
        let task = sqlx::query_as::<_, ProviderTaskRecord>(provider_task_sql!("WHERE id = ?"))
            .bind(input.task_id)
            .fetch_one(&mut *tx)
            .await?;
        insert_actual_cost_once(&mut tx, &task, &input).await?;
        let updated = sqlx::query_as::<_, ProviderTaskRecord>(provider_task_sql!("WHERE id = ?"))
            .bind(input.task_id)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Some(updated))
    }

    pub async fn record_materialization_failure(
        &self,
        task_id: &str,
        retry_after_seconds: i64,
        error_code: &str,
    ) -> StoreResult<Option<ProviderTaskRecord>> {
        let retry = positive_duration(retry_after_seconds, "materialization retry")?;
        let result = sqlx::query(
            r#"
            UPDATE run_provider_tasks
            SET materialization_attempts = materialization_attempts + 1,
                materialization_next_retry_at = datetime('now', ?),
                last_error_code = ?,
                updated_at = current_timestamp
            WHERE id = ? AND state = 'result_ready'
              AND materialization_deadline_at > current_timestamp
            "#,
        )
        .bind(retry)
        .bind(error_code)
        .bind(task_id)
        .execute(self.pool())
        .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.provider_task(task_id).await.map(Some)
    }

    pub async fn complete_provider_task(
        &self,
        task_id: &str,
        expected_state: &str,
        error_code: Option<&str>,
    ) -> StoreResult<Option<ProviderTaskRecord>> {
        transition_provider_task(
            self,
            task_id,
            expected_state,
            PROVIDER_TASK_COMPLETED,
            error_code,
        )
        .await
    }

    pub async fn cancel_provider_task(
        &self,
        task_id: &str,
        error_code: Option<&str>,
    ) -> StoreResult<Option<ProviderTaskRecord>> {
        transition_provider_task(
            self,
            task_id,
            PROVIDER_TASK_ACTIVE,
            PROVIDER_TASK_CANCELLED,
            error_code,
        )
        .await
    }

    pub async fn abandon_provider_task(
        &self,
        task_id: &str,
        expected_state: &str,
        error_code: &str,
    ) -> StoreResult<Option<ProviderTaskRecord>> {
        if expected_state == PROVIDER_TASK_RESULT_READY {
            return Err(StoreError::RecoveryInvariant {
                operation: "abandon_provider_task",
                message: "result_ready tasks are provider-completed".to_owned(),
            });
        }
        transition_provider_task(
            self,
            task_id,
            expected_state,
            PROVIDER_TASK_ABANDONED,
            Some(error_code),
        )
        .await
    }

    pub async fn link_run_step_output(
        &self,
        run_step_id: &str,
        port: &str,
        artifact_id: &str,
    ) -> StoreResult<RunStepOutputRecord> {
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
        .execute(self.pool())
        .await?;
        let output = sqlx::query_as::<_, RunStepOutputRecord>(
            r#"
            SELECT run_step_id, port, artifact_id, created_at
            FROM run_step_outputs
            WHERE run_step_id = ? AND port = ?
            "#,
        )
        .bind(run_step_id)
        .bind(port)
        .fetch_one(self.pool())
        .await?;
        if output.artifact_id != artifact_id {
            return Err(StoreError::RecoveryInvariant {
                operation: "link_run_step_output",
                message: format!(
                    "step `{run_step_id}` port `{port}` already maps to another artifact"
                ),
            });
        }
        Ok(output)
    }

    pub async fn run_step_outputs(&self, run_id: &str) -> StoreResult<Vec<RunStepOutputRecord>> {
        Ok(sqlx::query_as::<_, RunStepOutputRecord>(
            r#"
            SELECT output.run_step_id, output.port, output.artifact_id, output.created_at
            FROM run_step_outputs AS output
            JOIN run_steps AS step ON step.id = output.run_step_id
            WHERE step.run_id = ?
            ORDER BY step.rowid, output.port
            "#,
        )
        .bind(run_id)
        .fetch_all(self.pool())
        .await?)
    }

    pub async fn claim_run_recovery_lease(
        &self,
        run_id: &str,
        owner_id: &str,
        lease_seconds: i64,
    ) -> StoreResult<Option<RunRecoveryLeaseRecord>> {
        let lease = positive_duration(lease_seconds, "run recovery lease")?;
        sqlx::query(
            r#"
            INSERT INTO run_recovery_leases (
                run_id, owner_id, lease_expires_at, updated_at
            )
            SELECT ?, ?, datetime('now', ?), current_timestamp
            WHERE EXISTS (
                SELECT 1 FROM runs
                WHERE id = ? AND status IN ('queued', 'estimating', 'running')
            )
            ON CONFLICT(run_id) DO UPDATE SET
                owner_id = excluded.owner_id,
                lease_expires_at = excluded.lease_expires_at,
                updated_at = current_timestamp
            WHERE run_recovery_leases.owner_id = excluded.owner_id
               OR run_recovery_leases.lease_expires_at <= current_timestamp
            "#,
        )
        .bind(run_id)
        .bind(owner_id)
        .bind(lease)
        .bind(run_id)
        .execute(self.pool())
        .await?;
        Ok(sqlx::query_as::<_, RunRecoveryLeaseRecord>(
            r#"
            SELECT run_id, owner_id, lease_expires_at, updated_at
            FROM run_recovery_leases
            WHERE run_id = ? AND owner_id = ? AND lease_expires_at > current_timestamp
            "#,
        )
        .bind(run_id)
        .bind(owner_id)
        .fetch_optional(self.pool())
        .await?)
    }

    pub async fn create_or_read_execution_intent(
        &self,
        run_id: &str,
        plan_fingerprint: &str,
        estimate_fingerprint: &str,
        cost_decision: &str,
    ) -> StoreResult<RunExecutionIntentRecord> {
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO run_execution_intents (
                run_id, plan_fingerprint, estimate_fingerprint, cost_decision, created_at
            )
            VALUES (?, ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(run_id)
        .bind(plan_fingerprint)
        .bind(estimate_fingerprint)
        .bind(cost_decision)
        .execute(self.pool())
        .await?;
        let intent = sqlx::query_as::<_, RunExecutionIntentRecord>(
            r#"
            SELECT run_id, plan_fingerprint, estimate_fingerprint, cost_decision, created_at
            FROM run_execution_intents
            WHERE run_id = ?
            "#,
        )
        .bind(run_id)
        .fetch_one(self.pool())
        .await?;
        if intent.plan_fingerprint != plan_fingerprint
            || intent.estimate_fingerprint != estimate_fingerprint
            || intent.cost_decision != cost_decision
        {
            return Err(StoreError::RecoveryInvariant {
                operation: "create_or_read_execution_intent",
                message: format!("run `{run_id}` execution intent changed"),
            });
        }
        Ok(intent)
    }

    pub async fn run_execution_intent(
        &self,
        run_id: &str,
    ) -> StoreResult<Option<RunExecutionIntentRecord>> {
        Ok(sqlx::query_as::<_, RunExecutionIntentRecord>(
            r#"
            SELECT run_id, plan_fingerprint, estimate_fingerprint, cost_decision, created_at
            FROM run_execution_intents
            WHERE run_id = ?
            "#,
        )
        .bind(run_id)
        .fetch_optional(self.pool())
        .await?)
    }

    pub async fn request_run_terminalization(
        &self,
        run_id: &str,
        desired_status: &str,
        error_json: Option<&str>,
    ) -> StoreResult<RunTerminalizationRecord> {
        if !matches!(desired_status, "failed" | "interrupted") {
            return Err(StoreError::RecoveryInvariant {
                operation: "request_run_terminalization",
                message: format!("unsupported desired status `{desired_status}`"),
            });
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
        .bind(run_id)
        .bind(desired_status)
        .bind(error_json)
        .execute(self.pool())
        .await?;
        Ok(sqlx::query_as::<_, RunTerminalizationRecord>(
            r#"
            SELECT run_id, desired_status, state, error_json, owner_id,
                   lease_expires_at, created_at, updated_at, completed_at
            FROM run_terminalization_work_items
            WHERE run_id = ?
            "#,
        )
        .bind(run_id)
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn pending_run_terminalizations(&self) -> StoreResult<Vec<RunTerminalizationRecord>> {
        Ok(sqlx::query_as::<_, RunTerminalizationRecord>(
            r#"
            SELECT run_id, desired_status, state, error_json, owner_id,
                   lease_expires_at, created_at, updated_at, completed_at
            FROM run_terminalization_work_items
            WHERE state = 'settling'
            ORDER BY created_at, run_id
            "#,
        )
        .fetch_all(self.pool())
        .await?)
    }
}

async fn insert_actual_cost_once(
    tx: &mut Transaction<'_, Sqlite>,
    task: &ProviderTaskRecord,
    input: &ProviderTaskResult<'_>,
) -> StoreResult<()> {
    let operation_key = format!("actual:{}", task.run_step_id);
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO cost_ledger (
            id, workspace_id, run_id, run_step_id, provider,
            amount, currency, estimated, operation_key, created_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, current_timestamp)
        "#,
    )
    .bind(new_id("cost"))
    .bind(input.workspace_id)
    .bind(&task.run_id)
    .bind(&task.run_step_id)
    .bind(input.provider)
    .bind(input.amount)
    .bind(input.currency)
    .bind(if input.estimated { 1_i64 } else { 0_i64 })
    .bind(&operation_key)
    .execute(&mut **tx)
    .await?;
    let row = sqlx::query(
        r#"
        SELECT workspace_id, run_id, run_step_id, provider, amount, currency, estimated
        FROM cost_ledger
        WHERE operation_key = ?
        "#,
    )
    .bind(&operation_key)
    .fetch_one(&mut **tx)
    .await?;
    let matches = row.try_get::<String, _>("workspace_id")? == input.workspace_id
        && row.try_get::<Option<String>, _>("run_id")?.as_deref() == Some(task.run_id.as_str())
        && row.try_get::<Option<String>, _>("run_step_id")?.as_deref()
            == Some(task.run_step_id.as_str())
        && row.try_get::<String, _>("provider")? == input.provider
        && row.try_get::<f64, _>("amount")? == input.amount
        && row.try_get::<String, _>("currency")? == input.currency
        && (row.try_get::<i64, _>("estimated")? != 0) == input.estimated;
    if !matches {
        return Err(StoreError::RecoveryInvariant {
            operation: "insert_actual_cost_once",
            message: format!(
                "actual cost operation `{operation_key}` was reused with different data"
            ),
        });
    }
    Ok(())
}

async fn transition_provider_task(
    store: &Store,
    task_id: &str,
    expected_state: &str,
    next_state: &str,
    error_code: Option<&str>,
) -> StoreResult<Option<ProviderTaskRecord>> {
    let result = sqlx::query(
        r#"
        UPDATE run_provider_tasks
        SET state = ?,
            last_error_code = ?,
            ended_at = current_timestamp,
            updated_at = current_timestamp
        WHERE id = ? AND state = ?
        "#,
    )
    .bind(next_state)
    .bind(error_code)
    .bind(task_id)
    .bind(expected_state)
    .execute(store.pool())
    .await?;
    if result.rows_affected() == 0 {
        return Ok(None);
    }
    store.provider_task(task_id).await.map(Some)
}

fn positive_duration(seconds: i64, label: &'static str) -> StoreResult<String> {
    if seconds <= 0 {
        return Err(StoreError::RecoveryInvariant {
            operation: "positive_duration",
            message: format!("{label} must be positive"),
        });
    }
    Ok(format!("+{seconds} seconds"))
}
