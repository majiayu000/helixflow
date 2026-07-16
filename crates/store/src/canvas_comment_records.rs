use super::{Store, StoreError, StoreResult};

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct CanvasCommentStateRecord {
    pub workspace_id: String,
    pub seq: i64,
    pub comments_json: String,
    pub migration_source: String,
}

#[derive(Debug, Clone)]
pub struct NewCanvasCommentState<'a> {
    pub workspace_id: &'a str,
    pub seq: i64,
    pub comments_json: &'a str,
    pub migration_source: &'a str,
}

#[derive(Debug, Clone)]
pub struct CommitCanvasCommentOperation<'a> {
    pub workspace_id: &'a str,
    pub operation_id: &'a str,
    pub operation_fingerprint: &'a str,
    pub expected_seq: i64,
    pub comments_json: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasCommentCommitResult {
    Applied(CanvasCommentStateRecord),
    Replayed(CanvasCommentStateRecord),
    Stale { current_seq: i64 },
    OperationIdConflict { current_seq: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct CanvasCommentOperationRecord {
    pub operation_fingerprint: String,
    pub committed_seq: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct PersistedCanvasCommentState {
    workspace_id: String,
    seq: i64,
    version_seq: i64,
    comments_json: String,
    migration_source: String,
}

impl PersistedCanvasCommentState {
    fn logical_seq(&self) -> i64 {
        self.seq.max(self.version_seq)
    }

    fn into_record(self) -> CanvasCommentStateRecord {
        CanvasCommentStateRecord {
            workspace_id: self.workspace_id,
            seq: self.seq.max(self.version_seq),
            comments_json: self.comments_json,
            migration_source: self.migration_source,
        }
    }
}

impl Store {
    pub async fn initialize_canvas_comment_state(
        &self,
        input: NewCanvasCommentState<'_>,
    ) -> StoreResult<CanvasCommentStateRecord> {
        if input.seq < 0 {
            return Err(StoreError::CanvasCommentInvariant {
                workspace_id: input.workspace_id.to_owned(),
                message: "initial sequence must not be negative".to_owned(),
            });
        }
        if !matches!(input.migration_source, "empty" | "legacy_json") {
            return Err(StoreError::CanvasCommentInvariant {
                workspace_id: input.workspace_id.to_owned(),
                message: "migration source is invalid".to_owned(),
            });
        }

        sqlx::query(
            r#"
            INSERT INTO canvas_comment_states (
                workspace_id, seq, comments_json, migration_source, updated_at
            )
            SELECT w.id,
                   CASE WHEN ? > COALESCE(v.idx, 0) THEN ? ELSE COALESCE(v.idx, 0) END,
                   ?, ?, current_timestamp
            FROM workspaces w
            LEFT JOIN versions v ON v.id = w.cur_version_id
            WHERE w.id = ?
            ON CONFLICT(workspace_id) DO NOTHING
            "#,
        )
        .bind(input.seq)
        .bind(input.seq)
        .bind(input.comments_json)
        .bind(input.migration_source)
        .bind(input.workspace_id)
        .execute(self.pool())
        .await?;

        self.canvas_comment_state(input.workspace_id)
            .await?
            .ok_or_else(|| StoreError::CanvasCommentStateMissing {
                workspace_id: input.workspace_id.to_owned(),
            })
    }

    pub async fn canvas_comment_state(
        &self,
        workspace_id: &str,
    ) -> StoreResult<Option<CanvasCommentStateRecord>> {
        Ok(load_persisted_state(self.pool(), workspace_id)
            .await?
            .map(PersistedCanvasCommentState::into_record))
    }

    pub async fn canvas_comment_operation(
        &self,
        workspace_id: &str,
        operation_id: &str,
    ) -> StoreResult<Option<CanvasCommentOperationRecord>> {
        Ok(sqlx::query_as::<_, CanvasCommentOperationRecord>(
            r#"
            SELECT operation_fingerprint, committed_seq
            FROM canvas_comment_operations
            WHERE workspace_id = ? AND operation_id = ? AND committed_seq IS NOT NULL
            "#,
        )
        .bind(workspace_id)
        .bind(operation_id)
        .fetch_optional(self.pool())
        .await?)
    }

    pub async fn commit_canvas_comment_operation(
        &self,
        input: CommitCanvasCommentOperation<'_>,
    ) -> StoreResult<CanvasCommentCommitResult> {
        let mut tx = self.pool().begin().await?;
        let insert = sqlx::query(
            r#"
            INSERT INTO canvas_comment_operations (
                workspace_id, operation_id, operation_fingerprint, committed_seq, created_at
            ) VALUES (?, ?, ?, NULL, current_timestamp)
            ON CONFLICT(workspace_id, operation_id) DO NOTHING
            "#,
        )
        .bind(input.workspace_id)
        .bind(input.operation_id)
        .bind(input.operation_fingerprint)
        .execute(&mut *tx)
        .await?;

        if insert.rows_affected() == 0 {
            let existing = sqlx::query_as::<_, CanvasCommentOperationRecord>(
                r#"
                SELECT operation_fingerprint, committed_seq
                FROM canvas_comment_operations
                WHERE workspace_id = ? AND operation_id = ? AND committed_seq IS NOT NULL
                "#,
            )
            .bind(input.workspace_id)
            .bind(input.operation_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| StoreError::CanvasCommentInvariant {
                workspace_id: input.workspace_id.to_owned(),
                message: format!(
                    "operation `{}` exists without a committed sequence",
                    input.operation_id
                ),
            })?;
            let state = load_persisted_state(&mut *tx, input.workspace_id)
                .await?
                .ok_or_else(|| StoreError::CanvasCommentStateMissing {
                    workspace_id: input.workspace_id.to_owned(),
                })?
                .into_record();
            tx.commit().await?;
            if existing.operation_fingerprint == input.operation_fingerprint {
                return Ok(CanvasCommentCommitResult::Replayed(state));
            }
            return Ok(CanvasCommentCommitResult::OperationIdConflict {
                current_seq: state.seq,
            });
        }

        let persisted = load_persisted_state(&mut *tx, input.workspace_id)
            .await?
            .ok_or_else(|| StoreError::CanvasCommentStateMissing {
                workspace_id: input.workspace_id.to_owned(),
            })?;
        let current_seq = persisted.logical_seq();
        if input.expected_seq != current_seq {
            tx.rollback().await?;
            return Ok(CanvasCommentCommitResult::Stale { current_seq });
        }
        let next_seq =
            current_seq
                .checked_add(1)
                .ok_or_else(|| StoreError::CanvasCommentInvariant {
                    workspace_id: input.workspace_id.to_owned(),
                    message: "comment sequence overflow".to_owned(),
                })?;

        let update = sqlx::query(
            r#"
            UPDATE canvas_comment_states
            SET seq = ?, comments_json = ?, updated_at = current_timestamp
            WHERE workspace_id = ? AND seq = ?
            "#,
        )
        .bind(next_seq)
        .bind(input.comments_json)
        .bind(input.workspace_id)
        .bind(persisted.seq)
        .execute(&mut *tx)
        .await?;
        if update.rows_affected() != 1 {
            return Err(StoreError::CanvasCommentInvariant {
                workspace_id: input.workspace_id.to_owned(),
                message: "comment state changed inside serialized transaction".to_owned(),
            });
        }

        let operation_update = sqlx::query(
            r#"
            UPDATE canvas_comment_operations
            SET committed_seq = ?
            WHERE workspace_id = ? AND operation_id = ? AND committed_seq IS NULL
            "#,
        )
        .bind(next_seq)
        .bind(input.workspace_id)
        .bind(input.operation_id)
        .execute(&mut *tx)
        .await?;
        if operation_update.rows_affected() != 1 {
            return Err(StoreError::CanvasCommentInvariant {
                workspace_id: input.workspace_id.to_owned(),
                message: "operation record was not finalized".to_owned(),
            });
        }

        tx.commit().await?;
        Ok(CanvasCommentCommitResult::Applied(
            self.canvas_comment_state(input.workspace_id)
                .await?
                .ok_or_else(|| StoreError::CanvasCommentStateMissing {
                    workspace_id: input.workspace_id.to_owned(),
                })?,
        ))
    }
}

async fn load_persisted_state<'e, E>(
    executor: E,
    workspace_id: &str,
) -> Result<Option<PersistedCanvasCommentState>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query_as::<_, PersistedCanvasCommentState>(
        r#"
        SELECT s.workspace_id, s.seq, COALESCE(v.idx, 0) AS version_seq,
               s.comments_json, s.migration_source
        FROM canvas_comment_states s
        JOIN workspaces w ON w.id = s.workspace_id
        LEFT JOIN versions v ON v.id = w.cur_version_id
        WHERE s.workspace_id = ?
        "#,
    )
    .bind(workspace_id)
    .fetch_optional(executor)
    .await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[tokio::test]
    async fn canvas_comment_cas_is_atomic_and_idempotent() {
        let (_dir, store, workspace_id) = test_store().await;
        store
            .initialize_canvas_comment_state(NewCanvasCommentState {
                workspace_id: &workspace_id,
                seq: 0,
                comments_json: "[]",
                migration_source: "empty",
            })
            .await
            .expect("initialize comments");

        let applied = store
            .commit_canvas_comment_operation(commit_input(
                &workspace_id,
                "operation_1",
                "fingerprint_1",
                0,
                r#"[{"id":"comment_1"}]"#,
            ))
            .await
            .expect("apply operation");
        assert!(matches!(applied, CanvasCommentCommitResult::Applied(ref state) if state.seq == 1));

        let replayed = store
            .commit_canvas_comment_operation(commit_input(
                &workspace_id,
                "operation_1",
                "fingerprint_1",
                0,
                r#"[{"id":"ignored"}]"#,
            ))
            .await
            .expect("replay operation");
        assert!(
            matches!(replayed, CanvasCommentCommitResult::Replayed(ref state) if state.seq == 1)
        );

        let reused = store
            .commit_canvas_comment_operation(commit_input(
                &workspace_id,
                "operation_1",
                "different",
                1,
                "[]",
            ))
            .await
            .expect("detect operation reuse");
        assert_eq!(
            reused,
            CanvasCommentCommitResult::OperationIdConflict { current_seq: 1 }
        );

        let stale = store
            .commit_canvas_comment_operation(commit_input(
                &workspace_id,
                "operation_2",
                "fingerprint_2",
                0,
                "[]",
            ))
            .await
            .expect("detect stale base");
        assert_eq!(stale, CanvasCommentCommitResult::Stale { current_seq: 1 });
        assert!(
            store
                .canvas_comment_operation(&workspace_id, "operation_2")
                .await
                .expect("read rolled back operation")
                .is_none()
        );
        assert_eq!(
            store
                .canvas_comment_state(&workspace_id)
                .await
                .expect("read state")
                .expect("state exists")
                .comments_json,
            r#"[{"id":"comment_1"}]"#
        );
    }

    #[tokio::test]
    async fn forty_same_base_store_operations_have_one_winner_without_errors() {
        let (_dir, store, workspace_id) = test_store().await;
        store
            .initialize_canvas_comment_state(NewCanvasCommentState {
                workspace_id: &workspace_id,
                seq: 0,
                comments_json: "[]",
                migration_source: "empty",
            })
            .await
            .expect("initialize comments");
        let store = Arc::new(store);
        let mut tasks = Vec::new();
        for index in 0..40 {
            let store = Arc::clone(&store);
            let workspace_id = workspace_id.clone();
            tasks.push(tokio::spawn(async move {
                let operation_id = format!("operation_{index}");
                let fingerprint = format!("fingerprint_{index}");
                let comments = format!(r#"[{{"id":"comment_{index}"}}]"#);
                store
                    .commit_canvas_comment_operation(commit_input(
                        &workspace_id,
                        &operation_id,
                        &fingerprint,
                        0,
                        &comments,
                    ))
                    .await
            }));
        }

        let mut applied = 0;
        let mut stale = 0;
        for task in tasks {
            match task
                .await
                .expect("join operation")
                .expect("commit operation")
            {
                CanvasCommentCommitResult::Applied(_) => applied += 1,
                CanvasCommentCommitResult::Stale { current_seq: 1 } => stale += 1,
                result => panic!("unexpected commit result: {result:?}"),
            }
        }
        assert_eq!(applied, 1);
        assert_eq!(stale, 39);
    }

    fn commit_input<'a>(
        workspace_id: &'a str,
        operation_id: &'a str,
        operation_fingerprint: &'a str,
        expected_seq: i64,
        comments_json: &'a str,
    ) -> CommitCanvasCommentOperation<'a> {
        CommitCanvasCommentOperation {
            workspace_id,
            operation_id,
            operation_fingerprint,
            expected_seq,
            comments_json,
        }
    }

    async fn test_store() -> (tempfile::TempDir, Store, String) {
        let dir = tempfile::tempdir().expect("temp dir");
        let database_url = format!("sqlite://{}", dir.path().join("store.sqlite").display());
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store
            .create_workspace("Comments")
            .await
            .expect("create workspace");
        (dir, store, workspace.id)
    }
}
