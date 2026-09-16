use crate::{Store, StoreError, StoreResult, new_id};

#[derive(Debug, Clone)]
pub struct NewImageProcessingJob<'a> {
    pub workspace_id: &'a str,
    pub source_node_id: &'a str,
    pub intent: &'a str,
    pub profile: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct ImageProcessingJobUpdate<'a> {
    pub status: &'a str,
    pub provider_task_id: Option<&'a str>,
    pub provider: Option<&'a str>,
    pub model: Option<&'a str>,
    pub result_node_id: Option<&'a str>,
    pub output_upload_id: Option<&'a str>,
    pub error: Option<&'a str>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ImageProcessingJobRecord {
    pub id: String,
    pub workspace_id: String,
    pub source_node_id: String,
    pub result_node_id: Option<String>,
    pub intent: String,
    pub profile: Option<String>,
    pub provider_task_id: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub output_upload_id: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

impl Store {
    pub async fn create_image_processing_job(
        &self,
        input: NewImageProcessingJob<'_>,
    ) -> StoreResult<ImageProcessingJobRecord> {
        let id = new_id("imgjob");
        sqlx::query(
            r#"
            INSERT INTO image_processing_jobs (
                id, workspace_id, source_node_id, intent, profile,
                status, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, 'queued', current_timestamp, current_timestamp)
            "#,
        )
        .bind(&id)
        .bind(input.workspace_id)
        .bind(input.source_node_id)
        .bind(input.intent)
        .bind(input.profile)
        .execute(self.pool())
        .await?;
        self.workspace_image_processing_job(input.workspace_id, &id)
            .await
    }

    pub async fn update_image_processing_job(
        &self,
        workspace_id: &str,
        job_id: &str,
        input: ImageProcessingJobUpdate<'_>,
    ) -> StoreResult<ImageProcessingJobRecord> {
        let result = sqlx::query(
            r#"
            UPDATE image_processing_jobs
            SET status = ?,
                provider_task_id = COALESCE(?, provider_task_id),
                provider = COALESCE(?, provider),
                model = COALESCE(?, model),
                result_node_id = COALESCE(?, result_node_id),
                output_upload_id = COALESCE(?, output_upload_id),
                error = ?,
                updated_at = current_timestamp,
                completed_at = CASE
                    WHEN ? IN ('succeeded', 'failed', 'interrupted') THEN current_timestamp
                    ELSE NULL
                END
            WHERE id = ? AND workspace_id = ?
              AND (
                (status = 'queued' AND ? IN ('running', 'failed', 'interrupted'))
                OR
                (status = 'running' AND ? IN ('running', 'succeeded', 'failed', 'interrupted'))
              )
            "#,
        )
        .bind(input.status)
        .bind(input.provider_task_id)
        .bind(input.provider)
        .bind(input.model)
        .bind(input.result_node_id)
        .bind(input.output_upload_id)
        .bind(input.error)
        .bind(input.status)
        .bind(job_id)
        .bind(workspace_id)
        .bind(input.status)
        .bind(input.status)
        .execute(self.pool())
        .await?;
        if result.rows_affected() == 0 {
            let current = self
                .workspace_image_processing_job(workspace_id, job_id)
                .await?;
            return Err(StoreError::ImageProcessingJobStateConflict {
                job_id: job_id.to_owned(),
                actual_status: current.status,
                requested_status: input.status.to_owned(),
            });
        }
        self.workspace_image_processing_job(workspace_id, job_id)
            .await
    }

    pub async fn workspace_image_processing_jobs(
        &self,
        workspace_id: &str,
    ) -> StoreResult<Vec<ImageProcessingJobRecord>> {
        Ok(sqlx::query_as::<_, ImageProcessingJobRecord>(
            r#"
            SELECT id, workspace_id, source_node_id, result_node_id, intent,
                   profile, provider_task_id, provider, model, output_upload_id, status, error, created_at,
                   updated_at, completed_at
            FROM image_processing_jobs
            WHERE workspace_id = ?
            ORDER BY created_at DESC, id DESC
            LIMIT 50
            "#,
        )
        .bind(workspace_id)
        .fetch_all(self.pool())
        .await?)
    }

    pub async fn link_image_processing_result(
        &self,
        workspace_id: &str,
        job_id: &str,
        result_node_id: &str,
        output_upload_id: &str,
    ) -> StoreResult<ImageProcessingJobRecord> {
        let result = sqlx::query(
            r#"
            UPDATE image_processing_jobs
            SET result_node_id = ?, updated_at = current_timestamp
            WHERE id = ? AND workspace_id = ? AND status = 'succeeded'
              AND output_upload_id = ?
            "#,
        )
        .bind(result_node_id)
        .bind(job_id)
        .bind(workspace_id)
        .bind(output_upload_id)
        .execute(self.pool())
        .await?;
        if result.rows_affected() == 0 {
            let current = self
                .workspace_image_processing_job(workspace_id, job_id)
                .await?;
            return Err(StoreError::ImageProcessingJobStateConflict {
                job_id: job_id.to_owned(),
                actual_status: current.status,
                requested_status: "link_result".to_owned(),
            });
        }
        self.workspace_image_processing_job(workspace_id, job_id)
            .await
    }

    pub async fn workspace_image_processing_job(
        &self,
        workspace_id: &str,
        job_id: &str,
    ) -> StoreResult<ImageProcessingJobRecord> {
        Ok(sqlx::query_as::<_, ImageProcessingJobRecord>(
            r#"
            SELECT id, workspace_id, source_node_id, result_node_id, intent,
                   profile, provider_task_id, provider, model, output_upload_id, status, error, created_at,
                   updated_at, completed_at
            FROM image_processing_jobs
            WHERE id = ? AND workspace_id = ?
            "#,
        )
        .bind(job_id)
        .bind(workspace_id)
        .fetch_one(self.pool())
        .await?)
    }
}
