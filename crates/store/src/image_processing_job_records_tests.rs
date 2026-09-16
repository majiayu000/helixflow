use super::{ImageProcessingJobUpdate, NewImageProcessingJob, Store, StoreError};

#[tokio::test]
async fn image_processing_job_survives_reopen_and_keeps_provenance() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    let workspace = store
        .create_workspace("Images")
        .await
        .expect("create workspace");
    let job = store
        .create_image_processing_job(NewImageProcessingJob {
            workspace_id: &workspace.id,
            source_node_id: "photo",
            intent: "outpaint",
            profile: Some("gpt-image-2"),
        })
        .await
        .expect("create image job");
    assert_eq!(job.status, "queued");

    let running = store
        .update_image_processing_job(
            &workspace.id,
            &job.id,
            ImageProcessingJobUpdate {
                status: "running",
                provider_task_id: Some("provider-task-1"),
                provider: Some("atlascloud"),
                model: Some("openai/gpt-image-2/edit"),
                result_node_id: None,
                output_upload_id: None,
                error: None,
            },
        )
        .await
        .expect("start image job");
    assert_eq!(running.provider_task_id.as_deref(), Some("provider-task-1"));
    assert_eq!(running.provider.as_deref(), Some("atlascloud"));
    assert_eq!(running.model.as_deref(), Some("openai/gpt-image-2/edit"));

    let upload = store
        .create_upload(crate::NewUpload {
            workspace_id: &workspace.id,
            filename: "result.png",
            file_path: "uploads/result.png",
            sha256: "sha256:result",
            mime: Some("image/png"),
        })
        .await
        .expect("create result upload");
    store
        .update_image_processing_job(
            &workspace.id,
            &job.id,
            ImageProcessingJobUpdate {
                status: "succeeded",
                provider_task_id: Some("provider-task-1"),
                provider: None,
                model: None,
                result_node_id: None,
                output_upload_id: Some(&upload.id),
                error: None,
            },
        )
        .await
        .expect("complete image job");
    let linked = store
        .link_image_processing_result(&workspace.id, &job.id, "image_outpaint", &upload.id)
        .await
        .expect("link result node");
    assert_eq!(linked.status, "succeeded");
    assert_eq!(linked.result_node_id.as_deref(), Some("image_outpaint"));
    store.pool().close().await;

    let reopened = Store::open(&database_url).await.expect("reopen store");
    let jobs = reopened
        .workspace_image_processing_jobs(&workspace.id)
        .await
        .expect("list jobs");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].status, "succeeded");
    assert_eq!(jobs[0].result_node_id.as_deref(), Some("image_outpaint"));
    assert_eq!(
        jobs[0].output_upload_id.as_deref(),
        Some(upload.id.as_str())
    );
    assert!(jobs[0].completed_at.is_some());
}

#[tokio::test]
async fn image_processing_job_update_is_workspace_scoped() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    let owner = store.create_workspace("Owner").await.expect("create owner");
    let other = store.create_workspace("Other").await.expect("create other");
    let job = store
        .create_image_processing_job(NewImageProcessingJob {
            workspace_id: &owner.id,
            source_node_id: "photo",
            intent: "cutout",
            profile: None,
        })
        .await
        .expect("create image job");

    let error = store
        .update_image_processing_job(
            &other.id,
            &job.id,
            ImageProcessingJobUpdate {
                status: "failed",
                provider_task_id: None,
                provider: None,
                model: None,
                result_node_id: None,
                output_upload_id: None,
                error: Some("must stay private"),
            },
        )
        .await
        .expect_err("cross-workspace update must fail");
    assert!(error.is_not_found());
}

#[tokio::test]
async fn image_processing_job_terminal_state_cannot_be_rewritten() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    let workspace = store
        .create_workspace("Images")
        .await
        .expect("create workspace");
    let job = store
        .create_image_processing_job(NewImageProcessingJob {
            workspace_id: &workspace.id,
            source_node_id: "photo",
            intent: "enhance",
            profile: None,
        })
        .await
        .expect("create image job");
    store
        .update_image_processing_job(
            &workspace.id,
            &job.id,
            ImageProcessingJobUpdate {
                status: "failed",
                provider_task_id: None,
                provider: None,
                model: None,
                result_node_id: None,
                output_upload_id: None,
                error: Some("provider failed"),
            },
        )
        .await
        .expect("fail image job");

    let error = store
        .update_image_processing_job(
            &workspace.id,
            &job.id,
            ImageProcessingJobUpdate {
                status: "running",
                provider_task_id: Some("late-provider-task"),
                provider: Some("atlascloud"),
                model: Some("late-model"),
                result_node_id: None,
                output_upload_id: None,
                error: None,
            },
        )
        .await
        .expect_err("terminal state must be immutable");
    assert!(matches!(
        error,
        StoreError::ImageProcessingJobStateConflict {
            actual_status,
            requested_status,
            ..
        } if actual_status == "failed" && requested_status == "running"
    ));
}
