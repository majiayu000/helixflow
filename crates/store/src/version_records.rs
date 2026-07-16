use super::{NewVersion, Store, StoreError, StoreResult, VersionRecord, new_id};

impl Store {
    pub async fn create_version(&self, input: NewVersion<'_>) -> StoreResult<VersionRecord> {
        self.insert_version(input, None, false).await
    }

    pub async fn create_version_after(
        &self,
        input: NewVersion<'_>,
        expected_current_version_id: &str,
    ) -> StoreResult<VersionRecord> {
        self.insert_version(input, Some(expected_current_version_id), false)
            .await
    }

    pub async fn create_version_after_without_pending_proposal(
        &self,
        input: NewVersion<'_>,
        expected_current_version_id: &str,
    ) -> StoreResult<VersionRecord> {
        self.insert_version(input, Some(expected_current_version_id), true)
            .await
    }

    async fn insert_version(
        &self,
        input: NewVersion<'_>,
        expected_current_version_id: Option<&str>,
        reject_pending_proposal: bool,
    ) -> StoreResult<VersionRecord> {
        let version_id = new_id("ver");
        let mut tx = self.pool.begin().await?;

        if let Some(expected_version_id) = expected_current_version_id {
            let actual_version_id: Option<String> = sqlx::query_scalar(
                r#"
                SELECT cur_version_id
                FROM workspaces
                WHERE id = ?
                "#,
            )
            .bind(input.workspace_id)
            .fetch_one(&mut *tx)
            .await?;

            if actual_version_id.as_deref() != Some(expected_version_id) {
                return Err(StoreError::VersionConflict {
                    workspace_id: input.workspace_id.to_owned(),
                    expected_version_id: expected_version_id.to_owned(),
                    actual_version_id,
                });
            }
        }
        if reject_pending_proposal {
            let pending_proposal_id: Option<String> = sqlx::query_scalar(
                r#"
                SELECT id
                FROM proposals
                WHERE workspace_id = ? AND state = 'pending'
                ORDER BY created_at DESC, id DESC
                LIMIT 1
                "#,
            )
            .bind(input.workspace_id)
            .fetch_optional(&mut *tx)
            .await?;
            if pending_proposal_id.is_some() {
                return Err(StoreError::PendingProposalConflict {
                    workspace_id: input.workspace_id.to_owned(),
                });
            }
        }

        let idx: i64 = sqlx::query_scalar(
            r#"
            SELECT COALESCE(MAX(idx), 0) + 1
            FROM versions
            WHERE workspace_id = ?
            "#,
        )
        .bind(input.workspace_id)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO versions (
                id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(&version_id)
        .bind(input.workspace_id)
        .bind(idx)
        .bind(input.label)
        .bind(input.source.as_str())
        .bind(input.graph_path)
        .bind(input.graph_hash)
        .bind(input.parent_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE workspaces
            SET cur_version_id = ?, updated_at = current_timestamp
            WHERE id = ?
            "#,
        )
        .bind(&version_id)
        .bind(input.workspace_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        self.version(&version_id).await
    }

    pub async fn version(&self, version_id: &str) -> StoreResult<VersionRecord> {
        let version = sqlx::query_as::<_, VersionRecord>(
            r#"
            SELECT id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id, created_at
            FROM versions
            WHERE id = ?
            "#,
        )
        .bind(version_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(version)
    }
}
