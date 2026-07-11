use super::{RunRecord, Store, StoreResult};

impl Store {
    pub async fn latest_failed_workspace_run(
        &self,
        workspace_id: &str,
    ) -> StoreResult<Option<RunRecord>> {
        Ok(sqlx::query_as::<_, RunRecord>(
            r#"
            SELECT id, workspace_id, version_id, group_id, label, trigger, plan_json,
                   estimate_json, status, error_json, started_at, ended_at, created_at,
                   parent_run_id, attempt, force_rerun
            FROM runs
            WHERE workspace_id = ? AND status = 'failed'
            ORDER BY created_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(workspace_id)
        .fetch_optional(self.pool())
        .await?)
    }
}
