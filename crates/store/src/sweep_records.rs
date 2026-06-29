use super::{RunRecord, Store, StoreResult};

impl Store {
    pub async fn runs_for_group(&self, group_id: &str) -> StoreResult<Vec<RunRecord>> {
        Ok(sqlx::query_as::<_, RunRecord>(
            r#"
            SELECT id, workspace_id, version_id, group_id, label, trigger, plan_json,
                   estimate_json, status, error_json, started_at, ended_at, created_at
            FROM runs
            WHERE group_id = ?
            ORDER BY created_at, id
            "#,
        )
        .bind(group_id)
        .fetch_all(self.pool())
        .await?)
    }
}
