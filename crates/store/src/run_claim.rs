use super::{RunRecord, Store, StoreResult};

impl Store {
    /// Atomically transition a run's status, but only while its workspace has
    /// no other active run. The busy check and the compare-and-swap happen in
    /// a single UPDATE statement, so two concurrent claims for the same
    /// workspace cannot both pass the check (HF-018). Runs sharing `group_id`
    /// (sweeps) do not block each other, mirroring
    /// `RunService::ensure_workspace_not_busy`.
    pub async fn claim_run_if_workspace_idle(
        &self,
        run_id: &str,
        expected_status: &str,
        next_status: &str,
        workspace_id: &str,
        group_id: Option<&str>,
    ) -> StoreResult<Option<RunRecord>> {
        let result = sqlx::query(
            r#"
            UPDATE runs
            SET status = ?,
                started_at = CASE WHEN ? = 'running' AND started_at IS NULL THEN current_timestamp ELSE started_at END
            WHERE id = ? AND status = ?
              AND NOT EXISTS (
                SELECT 1 FROM runs AS other
                WHERE other.workspace_id = ?
                  AND other.id <> ?
                  AND other.status IN ('queued', 'estimating', 'running')
                  AND (? IS NULL OR other.group_id IS NULL OR other.group_id <> ?)
              )
            "#,
        )
        .bind(next_status)
        .bind(next_status)
        .bind(run_id)
        .bind(expected_status)
        .bind(workspace_id)
        .bind(run_id)
        .bind(group_id)
        .bind(group_id)
        .execute(self.pool())
        .await?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        self.run(run_id).await.map(Some)
    }
}
