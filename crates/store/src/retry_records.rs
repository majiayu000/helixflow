use sqlx::Row;

use super::run_recovery_records::run_in_transaction;
use super::{RunRecord, Store, StoreResult, new_id};

impl Store {
    /// Derive a detached retry run while preserving the failed parent for audit.
    /// Estimated ledger rows are copied so an over-budget retry shows its real
    /// confirmation amount before any steps have been recreated.
    pub async fn create_retry_run(
        &self,
        parent_run_id: &str,
        force_rerun: bool,
    ) -> StoreResult<RunRecord> {
        let parent = self.run(parent_run_id).await?;
        let id = new_id("run");
        let trigger = if parent.trigger == "sweep" {
            "agent"
        } else {
            &parent.trigger
        };
        let mut tx = self.pool().begin().await?;
        sqlx::query(
            r#"
            INSERT INTO runs (
                id, workspace_id, version_id, group_id, label, trigger, plan_json,
                estimate_json, status, parent_run_id, attempt, force_rerun, created_at
            )
            VALUES (?, ?, ?, NULL, ?, ?, ?, ?, 'waiting_confirmation', ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(&id)
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
            SELECT workspace_id, provider, amount, currency
            FROM cost_ledger
            WHERE run_id = ? AND estimated = 1
            ORDER BY created_at, id
            "#,
        )
        .bind(&parent.id)
        .fetch_all(&mut *tx)
        .await?;
        for estimate in estimates {
            sqlx::query(
                r#"
                INSERT INTO cost_ledger (
                    id, workspace_id, run_id, run_step_id, provider,
                    amount, currency, estimated, created_at
                )
                VALUES (?, ?, ?, NULL, ?, ?, ?, 1, current_timestamp)
                "#,
            )
            .bind(new_id("cost"))
            .bind(estimate.try_get::<String, _>("workspace_id")?)
            .bind(&id)
            .bind(estimate.try_get::<String, _>("provider")?)
            .bind(estimate.try_get::<f64, _>("amount")?)
            .bind(estimate.try_get::<String, _>("currency")?)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        self.run(&id).await
    }

    /// Get or create the single force-rerun child for a rejected output.
    ///
    /// Concurrent reject-and-rerun requests for the same parent serialize on
    /// `BEGIN IMMEDIATE` and share one child. This mapping is not stored in
    /// `run_failure_continuations` so succeeded-but-rejected runs cannot be
    /// pulled into failure self-heal.
    pub async fn create_reject_retry_run_once(
        &self,
        parent_run_id: &str,
    ) -> StoreResult<(RunRecord, bool)> {
        let mut tx = self.pool().begin_with("BEGIN IMMEDIATE").await?;
        let existing = sqlx::query_scalar::<_, String>(
            "SELECT child_run_id FROM run_reject_retries WHERE parent_run_id = ?",
        )
        .bind(parent_run_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(child_id) = existing {
            let child = run_in_transaction(&mut tx, &child_id).await?;
            tx.commit().await?;
            return Ok((child, false));
        }

        let parent = run_in_transaction(&mut tx, parent_run_id).await?;
        let child_id = new_id("run");
        let trigger = if parent.trigger == "sweep" {
            "agent"
        } else {
            parent.trigger.as_str()
        };
        sqlx::query(
            r#"
            INSERT INTO runs (
                id, workspace_id, version_id, group_id, label, trigger, plan_json,
                estimate_json, status, parent_run_id, attempt, force_rerun, created_at
            )
            VALUES (?, ?, ?, NULL, ?, ?, ?, ?, 'waiting_confirmation', ?, ?, 1,
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
            .bind(format!("reject-retry-estimate:{child_id}:{source_cost_id}"))
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            r#"
            INSERT INTO run_reject_retries (
                parent_run_id, child_run_id, retry_key, created_at
            )
            VALUES (?, ?, ?, current_timestamp)
            "#,
        )
        .bind(parent_run_id)
        .bind(&child_id)
        .bind(format!("reject:{parent_run_id}"))
        .execute(&mut *tx)
        .await?;

        let child = run_in_transaction(&mut tx, &child_id).await?;
        tx.commit().await?;
        Ok((child, true))
    }
}
