use crate::{Store, StoreResult, new_id};

#[derive(Debug, Clone)]
pub struct NewUpload<'a> {
    pub workspace_id: &'a str,
    pub filename: &'a str,
    pub file_path: &'a str,
    pub sha256: &'a str,
    pub mime: Option<&'a str>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UploadRecord {
    pub id: String,
    pub workspace_id: String,
    pub filename: String,
    pub file_path: String,
    pub sha256: String,
    pub mime: Option<String>,
    pub created_at: String,
}

impl Store {
    pub async fn create_upload(&self, input: NewUpload<'_>) -> StoreResult<UploadRecord> {
        let id = new_id("upload");
        sqlx::query(
            r#"
            INSERT INTO uploads (id, workspace_id, filename, file_path, sha256, mime, created_at)
            VALUES (?, ?, ?, ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(&id)
        .bind(input.workspace_id)
        .bind(input.filename)
        .bind(input.file_path)
        .bind(input.sha256)
        .bind(input.mime)
        .execute(self.pool())
        .await?;
        self.upload(&id).await
    }

    /// Look up an upload only within the given workspace. Fails closed
    /// (RowNotFound) when the id belongs to another workspace, so runs can
    /// never pull uploaded bytes across the workspace boundary.
    pub async fn workspace_upload(
        &self,
        workspace_id: &str,
        upload_id: &str,
    ) -> StoreResult<UploadRecord> {
        Ok(sqlx::query_as::<_, UploadRecord>(
            r#"
            SELECT id, workspace_id, filename, file_path, sha256, mime, created_at
            FROM uploads
            WHERE id = ? AND workspace_id = ?
            "#,
        )
        .bind(upload_id)
        .bind(workspace_id)
        .fetch_one(self.pool())
        .await?)
    }

    pub async fn upload(&self, upload_id: &str) -> StoreResult<UploadRecord> {
        Ok(sqlx::query_as::<_, UploadRecord>(
            r#"
            SELECT id, workspace_id, filename, file_path, sha256, mime, created_at
            FROM uploads
            WHERE id = ?
            "#,
        )
        .bind(upload_id)
        .fetch_one(self.pool())
        .await?)
    }
}
