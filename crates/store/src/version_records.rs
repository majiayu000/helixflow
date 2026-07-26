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
        if let Some(expected_version_id) = expected_current_version_id
            && input.parent_id != Some(expected_version_id)
        {
            return Err(StoreError::VersionParentMismatch {
                workspace_id: input.workspace_id.to_owned(),
                expected_parent_version_id: expected_version_id.to_owned(),
                actual_parent_version_id: input.parent_id.map(str::to_owned),
            });
        }
        let version_id = new_id("ver");
        let mut tx = if expected_current_version_id.is_some() {
            // Acquire the SQLite writer reservation before reading the next index. This makes
            // concurrent same-base writers wait for the committed CAS result instead of failing
            // a deferred read-to-write upgrade with SQLITE_BUSY.
            self.pool.begin_with("BEGIN IMMEDIATE").await?
        } else {
            self.pool.begin().await?
        };

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
                id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id,
                semantics_json, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, current_timestamp)
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
        .bind(input.semantics_json)
        .execute(&mut *tx)
        .await?;

        let current_update = match expected_current_version_id {
            Some(expected_version_id) if reject_pending_proposal => {
                sqlx::query(
                    r#"
                UPDATE workspaces
                SET cur_version_id = ?, updated_at = current_timestamp
                WHERE id = ?
                  AND cur_version_id = ?
                  AND NOT EXISTS (
                      SELECT 1
                      FROM proposals
                      WHERE workspace_id = workspaces.id AND state = 'pending'
                  )
                "#,
                )
                .bind(&version_id)
                .bind(input.workspace_id)
                .bind(expected_version_id)
                .execute(&mut *tx)
                .await?
            }
            Some(expected_version_id) => {
                sqlx::query(
                    r#"
                UPDATE workspaces
                SET cur_version_id = ?, updated_at = current_timestamp
                WHERE id = ? AND cur_version_id = ?
                "#,
                )
                .bind(&version_id)
                .bind(input.workspace_id)
                .bind(expected_version_id)
                .execute(&mut *tx)
                .await?
            }
            None => {
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
                .await?
            }
        };

        if current_update.rows_affected() == 0 {
            let expected_version_id =
                expected_current_version_id.ok_or(StoreError::StatementInvariant {
                    operation: "advance_unconditional_version",
                    expected_rows: 1,
                    actual_rows: 0,
                })?;
            let state: Option<(Option<String>, bool)> = sqlx::query_as(
                r#"
                SELECT
                    cur_version_id,
                    EXISTS (
                        SELECT 1
                        FROM proposals
                        WHERE workspace_id = workspaces.id AND state = 'pending'
                    )
                FROM workspaces
                WHERE id = ?
                "#,
            )
            .bind(input.workspace_id)
            .fetch_optional(&mut *tx)
            .await?;
            let (actual_version_id, has_pending_proposal) = state.unwrap_or((None, false));
            if actual_version_id.as_deref() != Some(expected_version_id) {
                return Err(StoreError::VersionConflict {
                    workspace_id: input.workspace_id.to_owned(),
                    expected_version_id: expected_version_id.to_owned(),
                    actual_version_id,
                });
            }
            if reject_pending_proposal && has_pending_proposal {
                return Err(StoreError::PendingProposalConflict {
                    workspace_id: input.workspace_id.to_owned(),
                });
            }
            return Err(StoreError::StatementInvariant {
                operation: "advance_version_after_atomic_predicate",
                expected_rows: 1,
                actual_rows: 0,
            });
        }
        if current_update.rows_affected() != 1 {
            return Err(StoreError::StatementInvariant {
                operation: "advance_version_after_atomic_predicate",
                expected_rows: 1,
                actual_rows: current_update.rows_affected(),
            });
        }

        tx.commit().await?;

        self.version(&version_id).await
    }

    pub async fn version(&self, version_id: &str) -> StoreResult<VersionRecord> {
        let version = sqlx::query_as::<_, VersionRecord>(
            r#"
            SELECT id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id,
                   semantics_json, created_at
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
