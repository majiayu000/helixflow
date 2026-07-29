use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use super::{Store, StoreError, StoreResult};

#[derive(Debug, Clone)]
pub struct NewArtifactPublishJournal<'a> {
    pub operation_key: &'a str,
    pub run_id: &'a str,
    pub run_step_id: &'a str,
    pub staged_path: &'a str,
    pub content_sha256: &'a str,
    pub owner_id: &'a str,
    pub expires_after_seconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, FromRow)]
pub struct ArtifactPublishJournalRecord {
    pub operation_key: String,
    pub run_id: String,
    pub run_step_id: String,
    pub staged_path: String,
    pub published_path: Option<String>,
    pub content_sha256: String,
    pub state: String,
    pub owner_id: String,
    pub expires_at: String,
    pub created_at: String,
    pub updated_at: String,
}

impl Store {
    pub async fn create_or_read_artifact_publish_journal(
        &self,
        input: NewArtifactPublishJournal<'_>,
    ) -> StoreResult<ArtifactPublishJournalRecord> {
        if input.expires_after_seconds <= 0 {
            return Err(StoreError::RecoveryInvariant {
                operation: "create_or_read_artifact_publish_journal",
                message: "journal expiry must be positive".to_owned(),
            });
        }
        let expiry = format!("+{} seconds", input.expires_after_seconds);
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO artifact_publish_journal (
                operation_key, run_id, run_step_id, staged_path, content_sha256,
                state, owner_id, expires_at, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, 'staged', ?, datetime('now', ?),
                    current_timestamp, current_timestamp)
            "#,
        )
        .bind(input.operation_key)
        .bind(input.run_id)
        .bind(input.run_step_id)
        .bind(input.staged_path)
        .bind(input.content_sha256)
        .bind(input.owner_id)
        .bind(expiry)
        .execute(self.pool())
        .await?;
        let record = self.artifact_publish_journal(input.operation_key).await?;
        if record.run_id != input.run_id
            || record.run_step_id != input.run_step_id
            || record.staged_path != input.staged_path
            || record.content_sha256 != input.content_sha256
        {
            return Err(StoreError::RecoveryInvariant {
                operation: "create_or_read_artifact_publish_journal",
                message: format!(
                    "operation `{}` was reused with different artifact data",
                    input.operation_key
                ),
            });
        }
        Ok(record)
    }

    pub async fn artifact_publish_journal(
        &self,
        operation_key: &str,
    ) -> StoreResult<ArtifactPublishJournalRecord> {
        Ok(sqlx::query_as::<_, ArtifactPublishJournalRecord>(
            r#"
            SELECT operation_key, run_id, run_step_id, staged_path, published_path,
                   content_sha256, state, owner_id, expires_at, created_at, updated_at
            FROM artifact_publish_journal
            WHERE operation_key = ?
            "#,
        )
        .bind(operation_key)
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn mark_artifact_published(
        &self,
        operation_key: &str,
        owner_id: &str,
        published_path: &str,
    ) -> StoreResult<Option<ArtifactPublishJournalRecord>> {
        let result = sqlx::query(
            r#"
            UPDATE artifact_publish_journal
            SET state = 'published',
                published_path = ?,
                updated_at = current_timestamp
            WHERE operation_key = ? AND owner_id = ? AND state = 'staged'
            "#,
        )
        .bind(published_path)
        .bind(operation_key)
        .bind(owner_id)
        .execute(self.pool())
        .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.artifact_publish_journal(operation_key).await.map(Some)
    }

    pub async fn mark_artifact_journal_committed(
        &self,
        operation_key: &str,
        owner_id: &str,
    ) -> StoreResult<Option<ArtifactPublishJournalRecord>> {
        let result = sqlx::query(
            r#"
            UPDATE artifact_publish_journal
            SET state = 'committed',
                updated_at = current_timestamp
            WHERE operation_key = ? AND owner_id = ? AND state = 'published'
            "#,
        )
        .bind(operation_key)
        .bind(owner_id)
        .execute(self.pool())
        .await?;
        if result.rows_affected() == 0 {
            return Ok(None);
        }
        self.artifact_publish_journal(operation_key).await.map(Some)
    }

    pub async fn pending_artifact_publish_journals(
        &self,
    ) -> StoreResult<Vec<ArtifactPublishJournalRecord>> {
        Ok(sqlx::query_as::<_, ArtifactPublishJournalRecord>(
            r#"
            SELECT operation_key, run_id, run_step_id, staged_path, published_path,
                   content_sha256, state, owner_id, expires_at, created_at, updated_at
            FROM artifact_publish_journal
            WHERE state IN ('staged', 'published')
            ORDER BY created_at, operation_key
            "#,
        )
        .fetch_all(self.pool())
        .await?)
    }
}
