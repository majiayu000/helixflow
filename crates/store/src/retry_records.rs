use sqlx::Row;

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
}
