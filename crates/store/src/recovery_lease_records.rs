use super::{RunRecoveryLeaseRecord, Store, StoreError, StoreResult};

impl Store {
    pub async fn run_recovery_lease(
        &self,
        run_id: &str,
    ) -> StoreResult<Option<RunRecoveryLeaseRecord>> {
        Ok(sqlx::query_as::<_, RunRecoveryLeaseRecord>(
            r#"
            SELECT run_id, owner_id, lease_expires_at, updated_at
            FROM run_recovery_leases
            WHERE run_id = ? AND lease_expires_at > current_timestamp
            "#,
        )
        .bind(run_id)
        .fetch_optional(self.pool())
        .await?)
    }

    pub async fn renew_run_recovery_lease(
        &self,
        run_id: &str,
        owner_id: &str,
        lease_seconds: i64,
    ) -> StoreResult<bool> {
        if lease_seconds <= 0 {
            return Err(StoreError::RecoveryInvariant {
                operation: "renew_run_recovery_lease",
                message: "lease must be positive".to_owned(),
            });
        }
        let lease = format!("+{lease_seconds} seconds");
        let result = sqlx::query(
            r#"
            UPDATE run_recovery_leases
            SET lease_expires_at = datetime('now', ?),
                updated_at = current_timestamp
            WHERE run_id = ? AND owner_id = ?
              AND lease_expires_at > current_timestamp
            "#,
        )
        .bind(lease)
        .bind(run_id)
        .bind(owner_id)
        .execute(self.pool())
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn release_run_recovery_lease(
        &self,
        run_id: &str,
        owner_id: &str,
    ) -> StoreResult<bool> {
        let result =
            sqlx::query("DELETE FROM run_recovery_leases WHERE run_id = ? AND owner_id = ?")
                .bind(run_id)
                .bind(owner_id)
                .execute(self.pool())
                .await?;
        Ok(result.rows_affected() == 1)
    }
}
