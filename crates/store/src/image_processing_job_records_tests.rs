use super::{
    ImageProcessingJobRecord, ImageProcessingJobUpdate, NewImageProcessingJob, Store, StoreError,
};

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

#[tokio::test]
async fn finalize_interrupted_image_processing_jobs_settles_only_active_rows() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    let workspace = store
        .create_workspace("Images")
        .await
        .expect("create workspace");
    let other = store
        .create_workspace("Other")
        .await
        .expect("create other workspace");
    let upload = store
        .create_upload(crate::NewUpload {
            workspace_id: &workspace.id,
            filename: "result.png",
            file_path: "uploads/result.png",
            sha256: "sha256:result",
            mime: Some("image/png"),
        })
        .await
        .expect("create upload");

    let queued = create_job(&store, &workspace.id, "enhance").await;
    let running = create_job(&store, &workspace.id, "upscale").await;
    store
        .update_image_processing_job(
            &workspace.id,
            &running.id,
            ImageProcessingJobUpdate {
                status: "running",
                provider_task_id: Some("provider-task-running"),
                provider: Some("atlas"),
                model: Some("atlascloud/photo-cleanup"),
                result_node_id: None,
                output_upload_id: Some(&upload.id),
                error: None,
            },
        )
        .await
        .expect("mark running");
    let succeeded = create_job(&store, &workspace.id, "cutout").await;
    store
        .update_image_processing_job(
            &workspace.id,
            &succeeded.id,
            job_update("running", Some("provider-task-succeeded"), None, None),
        )
        .await
        .expect("start succeeded job");
    store
        .update_image_processing_job(
            &workspace.id,
            &succeeded.id,
            job_update("succeeded", None, Some(&upload.id), None),
        )
        .await
        .expect("complete succeeded job");
    let succeeded_before = read_job(&store, &workspace.id, &succeeded.id).await;
    let failed = create_job(&store, &workspace.id, "outpaint").await;
    store
        .update_image_processing_job(
            &workspace.id,
            &failed.id,
            job_update("failed", None, None, Some("provider failed")),
        )
        .await
        .expect("fail job");
    let failed_before = read_job(&store, &workspace.id, &failed.id).await;
    let interrupted = create_job(&store, &workspace.id, "inpaint").await;
    store
        .update_image_processing_job(
            &workspace.id,
            &interrupted.id,
            job_update(
                "interrupted",
                Some("provider-task-old"),
                None,
                Some("already interrupted"),
            ),
        )
        .await
        .expect("interrupt job");
    let interrupted_before = read_job(&store, &workspace.id, &interrupted.id).await;
    let other_queued = create_job(&store, &other.id, "enhance").await;

    let settled = store
        .finalize_interrupted_image_processing_jobs()
        .await
        .expect("settle image jobs");
    assert_eq!(settled, 3);

    let queued = read_job(&store, &workspace.id, &queued.id).await;
    assert_eq!(queued.status, "interrupted");
    assert_eq!(queued.error.as_deref(), Some(RESTART_ERROR));
    assert!(queued.completed_at.is_some());
    assert!(queued.provider_task_id.is_none());
    assert!(queued.output_upload_id.is_none());

    let running = read_job(&store, &workspace.id, &running.id).await;
    assert_eq!(running.status, "interrupted");
    assert_eq!(running.error.as_deref(), Some(RESTART_ERROR));
    assert!(running.completed_at.is_some());
    assert_eq!(
        running.provider_task_id.as_deref(),
        Some("provider-task-running")
    );
    assert_eq!(
        running.output_upload_id.as_deref(),
        Some(upload.id.as_str())
    );
    assert_eq!(running.provider.as_deref(), Some("atlas"));
    assert_eq!(running.model.as_deref(), Some("atlascloud/photo-cleanup"));

    let succeeded_after = read_job(&store, &workspace.id, &succeeded.id).await;
    assert_eq!(succeeded_after.status, succeeded_before.status);
    assert_eq!(succeeded_after.error, succeeded_before.error);
    assert_eq!(succeeded_after.completed_at, succeeded_before.completed_at);
    assert_eq!(succeeded_after.updated_at, succeeded_before.updated_at);
    assert_eq!(
        succeeded_after.provider_task_id,
        succeeded_before.provider_task_id
    );
    assert_eq!(
        succeeded_after.output_upload_id,
        succeeded_before.output_upload_id
    );

    let failed_after = read_job(&store, &workspace.id, &failed.id).await;
    assert_eq!(failed_after.status, "failed");
    assert_eq!(failed_after.error, failed_before.error);
    assert_eq!(failed_after.completed_at, failed_before.completed_at);
    assert_eq!(failed_after.updated_at, failed_before.updated_at);

    let interrupted_after = read_job(&store, &workspace.id, &interrupted.id).await;
    assert_eq!(interrupted_after.status, "interrupted");
    assert_eq!(
        interrupted_after.error.as_deref(),
        Some("already interrupted")
    );
    assert_eq!(
        interrupted_after.provider_task_id,
        interrupted_before.provider_task_id
    );
    assert_eq!(
        interrupted_after.completed_at,
        interrupted_before.completed_at
    );
    assert_eq!(interrupted_after.updated_at, interrupted_before.updated_at);

    let other_queued = read_job(&store, &other.id, &other_queued.id).await;
    assert_eq!(other_queued.status, "interrupted");
    assert_eq!(other_queued.error.as_deref(), Some(RESTART_ERROR));

    assert_eq!(
        store
            .finalize_interrupted_image_processing_jobs()
            .await
            .expect("settle again"),
        0
    );
}

const RESTART_ERROR: &str = "image processing job was interrupted by a server restart";

async fn create_job(store: &Store, workspace_id: &str, intent: &str) -> ImageProcessingJobRecord {
    store
        .create_image_processing_job(NewImageProcessingJob {
            workspace_id,
            source_node_id: "photo",
            intent,
            profile: None,
        })
        .await
        .expect("create image job")
}

async fn read_job(store: &Store, workspace_id: &str, job_id: &str) -> ImageProcessingJobRecord {
    store
        .workspace_image_processing_job(workspace_id, job_id)
        .await
        .expect("read image job")
}

fn job_update<'a>(
    status: &'a str,
    provider_task_id: Option<&'a str>,
    output_upload_id: Option<&'a str>,
    error: Option<&'a str>,
) -> ImageProcessingJobUpdate<'a> {
    ImageProcessingJobUpdate {
        status,
        provider_task_id,
        provider: None,
        model: None,
        result_node_id: None,
        output_upload_id,
        error,
    }
}
