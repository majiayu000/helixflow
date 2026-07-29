use sqlx::Row;

use super::{CostLedgerRecord, NewCostLedger, Store, StoreError, StoreResult, new_id};

impl Store {
    pub async fn create_cost_ledger_once(
        &self,
        operation_key: &str,
        input: NewCostLedger<'_>,
    ) -> StoreResult<CostLedgerRecord> {
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
        .bind(input.run_id)
        .bind(input.run_step_id)
        .bind(input.provider)
        .bind(input.amount)
        .bind(input.currency)
        .bind(if input.estimated { 1_i64 } else { 0_i64 })
        .bind(operation_key)
        .execute(self.pool())
        .await?;
        let row = sqlx::query(
            r#"
            SELECT id, workspace_id, run_id, run_step_id, provider,
                   amount, currency, estimated
            FROM cost_ledger
            WHERE operation_key = ?
            "#,
        )
        .bind(operation_key)
        .fetch_one(self.pool())
        .await?;
        let matches = row.try_get::<String, _>("workspace_id")? == input.workspace_id
            && row.try_get::<Option<String>, _>("run_id")?.as_deref() == input.run_id
            && row.try_get::<Option<String>, _>("run_step_id")?.as_deref() == input.run_step_id
            && row.try_get::<String, _>("provider")? == input.provider
            && row.try_get::<f64, _>("amount")? == input.amount
            && row.try_get::<String, _>("currency")? == input.currency
            && (row.try_get::<i64, _>("estimated")? != 0) == input.estimated;
        if !matches {
            return Err(StoreError::RecoveryInvariant {
                operation: "create_cost_ledger_once",
                message: format!("cost operation `{operation_key}` was reused with different data"),
            });
        }
        self.cost_ledger(&row.try_get::<String, _>("id")?).await
    }
}
