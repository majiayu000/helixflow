use super::run_records::artifact_from_row;
use super::{ArtifactRecord, RunRecord, Store, StoreResult};

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

    pub async fn artifacts_for_group(&self, group_id: &str) -> StoreResult<Vec<ArtifactRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT a.id, a.workspace_id, a.run_id, a.run_step_id, a.node_id, a.kind,
                   a.storage_uri, a.sha256, a.mime, a.width, a.height, a.duration_ms,
                   a.selected, a.meta_json, a.created_at, a.review_state
            FROM artifacts a
            INNER JOIN runs r ON r.id = a.run_id
            WHERE r.group_id = ?
            ORDER BY r.created_at, r.id, a.created_at, a.id
            "#,
        )
        .bind(group_id)
        .fetch_all(self.pool())
        .await?;

        rows.into_iter().map(artifact_from_row).collect()
    }

    pub async fn select_group_artifact(
        &self,
        artifact_id: &str,
        group_id: &str,
    ) -> StoreResult<ArtifactRecord> {
        let mut tx = self.pool().begin().await?;
        sqlx::query(
            r#"
            UPDATE artifacts
            SET selected = CASE WHEN id = ? THEN 1 ELSE 0 END
            WHERE run_id IN (
                SELECT id
                FROM runs
                WHERE group_id = ?
            )
            "#,
        )
        .bind(artifact_id)
        .bind(group_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        self.artifact(artifact_id).await
    }
}
