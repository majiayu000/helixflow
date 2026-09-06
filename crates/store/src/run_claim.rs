use super::{NewRun, RunRecord, Store, StoreError, StoreResult, new_id};

impl Store {
    pub async fn create_run(&self, input: NewRun<'_>) -> StoreResult<RunRecord> {
        let id = new_id("run");
        // Reserve the writer before reading admission state so concurrent creates
        // and claims observe this insert before taking the workspace.
        let mut tx = self.pool().begin_with("BEGIN IMMEDIATE").await?;
        let actual_workspace_id: Option<String> = sqlx::query_scalar(
            r#"
            SELECT workspace_id
            FROM versions
            WHERE id = ?
            "#,
        )
        .bind(input.version_id)
        .fetch_optional(&mut *tx)
        .await?;
        if actual_workspace_id.as_deref() != Some(input.workspace_id) {
            return Err(StoreError::RunVersionMismatch {
                workspace_id: input.workspace_id.to_owned(),
                version_id: input.version_id.to_owned(),
                actual_workspace_id,
            });
        }

        if matches!(input.status, "queued" | "estimating" | "running") {
            let active_run_id: Option<String> = sqlx::query_scalar(
                r#"
                SELECT id FROM runs
                WHERE workspace_id = ?
                  AND status IN ('queued', 'estimating', 'running')
                  AND (? IS NULL OR group_id IS NULL OR group_id <> ?)
                LIMIT 1
                "#,
            )
            .bind(input.workspace_id)
            .bind(input.group_id)
            .bind(input.group_id)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some(active_run_id) = active_run_id {
                return Err(StoreError::WorkspaceBusy {
                    workspace_id: input.workspace_id.to_owned(),
                    active_run_id,
                });
            }
        }

        sqlx::query(
            r#"
            INSERT INTO runs (
                id, workspace_id, version_id, group_id, label, trigger, plan_json,
                estimate_json, status, force_rerun, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(&id)
        .bind(input.workspace_id)
        .bind(input.version_id)
        .bind(input.group_id)
        .bind(input.label)
        .bind(input.trigger)
        .bind(input.plan_json)
        .bind(input.estimate_json)
        .bind(input.status)
        .bind(0_i64)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        self.run(&id).await
    }

    pub async fn active_workspace_runs(&self, workspace_id: &str) -> StoreResult<Vec<RunRecord>> {
        Ok(sqlx::query_as::<_, RunRecord>(
            r#"
            SELECT id, workspace_id, version_id, group_id, label, trigger, plan_json,
                   estimate_json, status, error_json, started_at, ended_at, created_at,
                   parent_run_id, attempt, force_rerun
            FROM runs
            WHERE workspace_id = ? AND status IN ('queued', 'estimating', 'running')
            ORDER BY created_at, id
            "#,
        )
        .bind(workspace_id)
        .fetch_all(self.pool())
        .await?)
    }

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
