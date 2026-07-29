use tempfile::TempDir;

use super::{
    NewProviderTask, PROVIDER_TASK_ACTIVE, PROVIDER_TASK_CANCELLED, PROVIDER_TASK_COMPLETED,
    PROVIDER_TASK_DISPATCHING, PROVIDER_TASK_RESULT_READY, ProviderTaskFailureFinalization,
    ProviderTaskHandleUpdate, ProviderTaskResult, Store,
};
use crate::NewArtifactPublishJournal;
use crate::{
    NewArtifact, NewCostLedger, NewRun, NewRunStep, NewVersion, StoreError, VersionSource,
};

struct RecoveryFixture {
    store: Store,
    temp_dir: TempDir,
    database_url: String,
    workspace_id: String,
    run_id: String,
    step_id: String,
}

impl RecoveryFixture {
    async fn create() -> Self {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let database_url = format!(
            "sqlite://{}",
            temp_dir.path().join("helixflow.sqlite").display()
        );
        let store = Store::open(&database_url).await.expect("open store");
        let workspace = store
            .create_workspace("Recovery")
            .await
            .expect("create workspace");
        let version = store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "Graph",
                source: VersionSource::Manual,
                graph_path: "graphs/current.json",
                graph_hash: "sha256:graph",
                parent_id: None,
                semantics_json: None,
            })
            .await
            .expect("create version");
        let run = store
            .create_run(NewRun {
                workspace_id: &workspace.id,
                version_id: &version.id,
                group_id: None,
                label: "Recovery",
                trigger: "manual",
                plan_json: Some("{}"),
                estimate_json: Some("{}"),
                status: "running",
            })
            .await
            .expect("create run");
        let step = store
            .create_run_step(NewRunStep {
                run_id: &run.id,
                node_id: "video",
                node_type: "video.generate",
                provider: Some("atlas"),
                state: "running",
            })
            .await
            .expect("create step");
        Self {
            store,
            temp_dir,
            database_url,
            workspace_id: workspace.id,
            run_id: run.id,
            step_id: step.id,
        }
    }

    async fn dispatching(&self) -> crate::ProviderTaskRecord {
        self.store
            .insert_or_read_provider_task(NewProviderTask {
                run_id: &self.run_id,
                run_step_id: &self.step_id,
                provider: "atlas",
                dispatch_origin: "https://api.atlascloud.ai",
                recovery_scope_fingerprint: "sha256:scope",
                operation_key: "dispatch:step",
                dispatch_owner_id: "owner-a",
                dispatch_lease_seconds: 30,
                dispatch_deadline_seconds: 60,
            })
            .await
            .expect("insert dispatch intent")
    }
}

#[tokio::test]
async fn provider_handle_survives_store_reopen() {
    let fixture = RecoveryFixture::create().await;
    let task = fixture.dispatching().await;
    assert_eq!(task.state, PROVIDER_TASK_DISPATCHING);
    let active = fixture
        .store
        .activate_provider_task(ProviderTaskHandleUpdate {
            task_id: &task.id,
            dispatch_owner_id: "owner-a",
            provider_task_id: "prediction-123",
            status_url: Some("https://api.atlascloud.ai/api/v1/model/prediction/123"),
            result_url: None,
            recovery_deadline_seconds: 600,
        })
        .await
        .expect("activate task")
        .expect("owner wins CAS");
    assert_eq!(active.state, PROVIDER_TASK_ACTIVE);

    fixture.store.pool().close().await;
    let reopened = Store::open(&fixture.database_url)
        .await
        .expect("reopen store");
    let recovered = reopened
        .provider_task_for_step(&fixture.step_id)
        .await
        .expect("read task")
        .expect("durable task");
    assert_eq!(
        recovered.provider_task_id.as_deref(),
        Some("prediction-123")
    );
    assert_eq!(recovered.dispatch_origin, "https://api.atlascloud.ai");
    assert_eq!(recovered.recovery_scope_fingerprint, "sha256:scope");
    drop(fixture.temp_dir);
}

#[tokio::test]
async fn provider_task_identity_is_step_unique_and_immutable() {
    let fixture = RecoveryFixture::create().await;
    let first = fixture.dispatching().await;
    let replay = fixture.dispatching().await;
    assert_eq!(first.id, replay.id);

    let err = fixture
        .store
        .insert_or_read_provider_task(NewProviderTask {
            run_id: &fixture.run_id,
            run_step_id: &fixture.step_id,
            provider: "fal",
            dispatch_origin: "https://queue.fal.run",
            recovery_scope_fingerprint: "sha256:other",
            operation_key: "dispatch:other",
            dispatch_owner_id: "owner-b",
            dispatch_lease_seconds: 30,
            dispatch_deadline_seconds: 60,
        })
        .await
        .expect_err("identity mismatch must fail");
    assert!(matches!(
        err,
        StoreError::RecoveryInvariant {
            operation: "insert_or_read_provider_task",
            ..
        }
    ));
}

#[tokio::test]
async fn provider_failure_atomically_settles_task_step_and_terminal_work() {
    let fixture = RecoveryFixture::create().await;
    let task = fixture.dispatching().await;
    let mut failure = ProviderTaskFailureFinalization {
        provider_task_id: &task.id,
        expected_task_state: PROVIDER_TASK_DISPATCHING,
        terminal_task_state: PROVIDER_TASK_COMPLETED,
        error_code: "PROVIDER_REJECTED",
        desired_run_status: "failed",
        error_json: None,
        required_expired_foreign_owner: Some("owner-b"),
    };
    assert!(
        !fixture
            .store
            .finalize_provider_task_failure(failure.clone())
            .await
            .expect("guard live dispatch owner")
    );
    failure.error_json = Some(r#"{"error":"provider execution failed"}"#);
    failure.required_expired_foreign_owner = None;
    assert!(
        fixture
            .store
            .finalize_provider_task_failure(failure.clone())
            .await
            .expect("finalize provider failure")
    );
    assert_eq!(
        fixture
            .store
            .provider_task(&task.id)
            .await
            .expect("read task")
            .state,
        PROVIDER_TASK_COMPLETED
    );
    assert_eq!(
        fixture
            .store
            .run_step(&fixture.step_id)
            .await
            .expect("read step")
            .state,
        "failed"
    );
    let work = fixture
        .store
        .pending_run_terminalizations()
        .await
        .expect("read terminal work");
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].desired_status, "failed");
    assert_eq!(
        fixture
            .store
            .run(&fixture.run_id)
            .await
            .expect("read active run")
            .status,
        "running"
    );
    assert!(
        fixture
            .store
            .finalize_provider_task_failure(failure.clone())
            .await
            .expect("replay provider failure")
    );
    failure.error_code = "DIFFERENT_FAILURE";
    failure.error_json = None;
    assert!(
        !fixture
            .store
            .finalize_provider_task_failure(failure)
            .await
            .expect("reject mismatched replay")
    );
}

#[tokio::test]
async fn only_dispatch_owner_can_persist_the_remote_handle() {
    let fixture = RecoveryFixture::create().await;
    let task = fixture.dispatching().await;
    let wrong = fixture
        .store
        .activate_provider_task(ProviderTaskHandleUpdate {
            task_id: &task.id,
            dispatch_owner_id: "owner-b",
            provider_task_id: "paid-task",
            status_url: None,
            result_url: None,
            recovery_deadline_seconds: 600,
        })
        .await
        .expect("attempt wrong CAS");
    assert!(wrong.is_none());
    assert!(
        fixture
            .store
            .renew_dispatch_owner(&task.id, "owner-a", 30)
            .await
            .expect("renew owner")
    );
    let active = fixture
        .store
        .activate_provider_task(ProviderTaskHandleUpdate {
            task_id: &task.id,
            dispatch_owner_id: "owner-a",
            provider_task_id: "paid-task",
            status_url: None,
            result_url: None,
            recovery_deadline_seconds: 600,
        })
        .await
        .expect("activate")
        .expect("matching owner wins");
    assert_eq!(active.provider_task_id.as_deref(), Some("paid-task"));
}

#[tokio::test]
async fn result_ready_and_actual_cost_commit_before_materialization() {
    let fixture = RecoveryFixture::create().await;
    let task = fixture.dispatching().await;
    let active = fixture
        .store
        .activate_provider_task(ProviderTaskHandleUpdate {
            task_id: &task.id,
            dispatch_owner_id: "owner-a",
            provider_task_id: "paid-task",
            status_url: None,
            result_url: None,
            recovery_deadline_seconds: 600,
        })
        .await
        .expect("activate")
        .expect("active task");
    let result = ProviderTaskResult {
        task_id: &active.id,
        dispatch_owner_id: None,
        terminal_outcome: "succeeded",
        result_spool_path: "spool/result.json",
        result_fingerprint: "sha256:result",
        materialization_deadline_seconds: 300,
        workspace_id: &fixture.workspace_id,
        provider: "atlas",
        amount: 1.25,
        currency: "USD",
        estimated: false,
    };
    let ready = fixture
        .store
        .mark_provider_task_result_ready(result.clone())
        .await
        .expect("mark ready")
        .expect("state transition");
    assert_eq!(ready.state, PROVIDER_TASK_RESULT_READY);
    assert_eq!(ready.materialization_attempts, 0);
    let costs = fixture
        .store
        .cost_ledger_for_run(&fixture.run_id)
        .await
        .expect("cost ledger");
    assert_eq!(costs.len(), 1);
    assert_eq!(costs[0].amount, 1.25);

    assert!(
        fixture
            .store
            .mark_provider_task_result_ready(result)
            .await
            .expect("replay ready")
            .is_none()
    );
    assert_eq!(
        fixture
            .store
            .cost_ledger_for_run(&fixture.run_id)
            .await
            .expect("cost ledger")
            .len(),
        1
    );
    let err = fixture
        .store
        .abandon_provider_task(&ready.id, PROVIDER_TASK_RESULT_READY, "UNKNOWN")
        .await
        .expect_err("completed provider cannot be abandoned");
    assert!(matches!(err, StoreError::RecoveryInvariant { .. }));

    let retry = fixture
        .store
        .record_materialization_failure(&ready.id, 5, "ARTIFACT_DOWNLOAD_FAILED")
        .await
        .expect("record retry")
        .expect("still ready");
    assert_eq!(retry.materialization_attempts, 1);
    let completed = fixture
        .store
        .complete_provider_task(&ready.id, PROVIDER_TASK_RESULT_READY, None)
        .await
        .expect("complete materialization")
        .expect("transition");
    assert_eq!(completed.state, PROVIDER_TASK_COMPLETED);
}

#[tokio::test]
async fn step_output_mapping_is_idempotent_and_rejects_conflicts() {
    let fixture = RecoveryFixture::create().await;
    let first = fixture
        .store
        .create_artifact(NewArtifact {
            workspace_id: &fixture.workspace_id,
            run_id: Some(&fixture.run_id),
            run_step_id: Some(&fixture.step_id),
            node_id: Some("video"),
            kind: "video",
            storage_uri: "artifacts/first.mp4",
            sha256: Some("sha256:first"),
            mime: Some("video/mp4"),
            width: None,
            height: None,
            duration_ms: None,
            selected: false,
            meta_json: None,
        })
        .await
        .expect("create first artifact");
    let second = fixture
        .store
        .create_artifact(NewArtifact {
            workspace_id: &fixture.workspace_id,
            run_id: Some(&fixture.run_id),
            run_step_id: Some(&fixture.step_id),
            node_id: Some("video"),
            kind: "video",
            storage_uri: "artifacts/second.mp4",
            sha256: Some("sha256:second"),
            mime: Some("video/mp4"),
            width: None,
            height: None,
            duration_ms: None,
            selected: false,
            meta_json: None,
        })
        .await
        .expect("create second artifact");
    let output = fixture
        .store
        .link_run_step_output(&fixture.step_id, "video", &first.id)
        .await
        .expect("link output");
    assert_eq!(output.artifact_id, first.id);
    let replay = fixture
        .store
        .link_run_step_output(&fixture.step_id, "video", &first.id)
        .await
        .expect("replay output");
    assert_eq!(replay, output);
    let err = fixture
        .store
        .link_run_step_output(&fixture.step_id, "video", &second.id)
        .await
        .expect_err("conflicting output must fail");
    assert!(matches!(err, StoreError::RecoveryInvariant { .. }));
}

#[tokio::test]
async fn recovery_lease_and_terminal_desire_are_monotonic() {
    let fixture = RecoveryFixture::create().await;
    let first = fixture
        .store
        .claim_run_recovery_lease(&fixture.run_id, "owner-a", 30)
        .await
        .expect("claim lease")
        .expect("owner-a lease");
    assert_eq!(first.owner_id, "owner-a");
    assert!(
        fixture
            .store
            .renew_run_recovery_lease(&fixture.run_id, "owner-a", 30)
            .await
            .expect("renew recovery lease")
    );
    assert!(
        fixture
            .store
            .claim_run_recovery_lease(&fixture.run_id, "owner-b", 30)
            .await
            .expect("competing claim")
            .is_none()
    );
    sqlx::query(
        "UPDATE run_recovery_leases SET lease_expires_at = datetime('now', '-1 second') WHERE run_id = ?",
    )
    .bind(&fixture.run_id)
    .execute(fixture.store.pool())
    .await
    .expect("expire lease");
    let second = fixture
        .store
        .claim_run_recovery_lease(&fixture.run_id, "owner-b", 30)
        .await
        .expect("claim expired lease")
        .expect("owner-b lease");
    assert_eq!(second.owner_id, "owner-b");

    let failed = fixture
        .store
        .request_run_terminalization(&fixture.run_id, "failed", Some(r#"{"code":"FAILED"}"#))
        .await
        .expect("request failed");
    assert_eq!(failed.desired_status, "failed");
    let interrupted = fixture
        .store
        .request_run_terminalization(&fixture.run_id, "interrupted", None)
        .await
        .expect("upgrade interrupt");
    assert_eq!(interrupted.desired_status, "interrupted");
    let cannot_downgrade = fixture
        .store
        .request_run_terminalization(&fixture.run_id, "failed", Some(r#"{"code":"LATE"}"#))
        .await
        .expect("late failed");
    assert_eq!(cannot_downgrade.desired_status, "interrupted");
    assert!(cannot_downgrade.error_json.is_none());
    assert!(
        fixture
            .store
            .claim_run_terminalization(&fixture.run_id, "settler-a", 30)
            .await
            .expect("claim terminalization")
    );
    assert!(
        fixture
            .store
            .renew_run_terminalization_lease(&fixture.run_id, "settler-a", 30)
            .await
            .expect("renew terminalization")
    );
    fixture
        .store
        .release_run_recovery_lease(&fixture.run_id, "owner-b")
        .await
        .expect("release recovery lease");
    fixture
        .store
        .update_run_status(&fixture.run_id, "interrupted", None)
        .await
        .expect("terminalize run");
    assert!(
        fixture
            .store
            .claim_run_recovery_lease(&fixture.run_id, "owner-c", 30)
            .await
            .expect("terminal claim")
            .is_none()
    );
}

#[tokio::test]
async fn execution_intent_replay_requires_identical_fingerprints() {
    let fixture = RecoveryFixture::create().await;
    let first = fixture
        .store
        .create_or_read_execution_intent(
            &fixture.run_id,
            "sha256:plan",
            "sha256:estimate",
            "approved",
        )
        .await
        .expect("create intent");
    let replay = fixture
        .store
        .create_or_read_execution_intent(
            &fixture.run_id,
            "sha256:plan",
            "sha256:estimate",
            "approved",
        )
        .await
        .expect("replay intent");
    assert_eq!(first, replay);
    let err = fixture
        .store
        .create_or_read_execution_intent(
            &fixture.run_id,
            "sha256:changed",
            "sha256:estimate",
            "approved",
        )
        .await
        .expect_err("changed intent must fail");
    assert!(matches!(err, StoreError::RecoveryInvariant { .. }));
}

#[tokio::test]
async fn terminal_provider_states_cannot_be_rewritten() {
    let fixture = RecoveryFixture::create().await;
    let task = fixture.dispatching().await;
    let active = fixture
        .store
        .activate_provider_task(ProviderTaskHandleUpdate {
            task_id: &task.id,
            dispatch_owner_id: "owner-a",
            provider_task_id: "paid-task",
            status_url: None,
            result_url: None,
            recovery_deadline_seconds: 600,
        })
        .await
        .expect("activate")
        .expect("active");
    let cancelled = fixture
        .store
        .cancel_provider_task(&active.id, None)
        .await
        .expect("cancel")
        .expect("cancelled");
    assert_eq!(cancelled.state, PROVIDER_TASK_CANCELLED);
    assert!(
        fixture
            .store
            .complete_provider_task(&active.id, PROVIDER_TASK_ACTIVE, None)
            .await
            .expect("late completion")
            .is_none()
    );
}

#[tokio::test]
async fn artifact_publish_journal_replays_each_state_exactly_once() {
    let fixture = RecoveryFixture::create().await;
    let input = NewArtifactPublishJournal {
        operation_key: "artifact:step:video",
        run_id: &fixture.run_id,
        run_step_id: &fixture.step_id,
        staged_path: "spool/video.part",
        content_sha256: "sha256:video",
        owner_id: "owner-a",
        expires_after_seconds: 300,
    };
    let staged = fixture
        .store
        .create_or_read_artifact_publish_journal(input.clone())
        .await
        .expect("stage journal");
    assert_eq!(staged.state, "staged");
    assert_eq!(
        fixture
            .store
            .create_or_read_artifact_publish_journal(input)
            .await
            .expect("replay stage"),
        staged
    );
    assert!(
        fixture
            .store
            .mark_artifact_published("artifact:step:video", "owner-b", "artifacts/video.mp4")
            .await
            .expect("wrong owner")
            .is_none()
    );
    let artifact_input = || NewArtifact {
        workspace_id: &fixture.workspace_id,
        run_id: Some(&fixture.run_id),
        run_step_id: Some(&fixture.step_id),
        node_id: Some("video"),
        kind: "video",
        storage_uri: "artifacts/video.mp4",
        sha256: Some("sha256:video"),
        mime: Some("video/mp4"),
        width: Some(720),
        height: Some(1280),
        duration_ms: Some(4000),
        selected: false,
        meta_json: Some("{}"),
    };
    let artifact = fixture
        .store
        .publish_artifact_from_journal(
            "artifact:step:video",
            "owner-a",
            "artifacts/video.mp4",
            artifact_input(),
        )
        .await
        .expect("publish artifact");
    let replay = fixture
        .store
        .publish_artifact_from_journal(
            "artifact:step:video",
            "owner-a",
            "artifacts/video.mp4",
            artifact_input(),
        )
        .await
        .expect("replay artifact");
    assert_eq!(artifact.id, replay.id);
    let published = fixture
        .store
        .artifact_publish_journal("artifact:step:video")
        .await
        .expect("read published journal");
    assert_eq!(published.state, "published");
    assert_eq!(published.artifact_id.as_deref(), Some(artifact.id.as_str()));
    assert_eq!(
        published.published_path.as_deref(),
        Some("artifacts/video.mp4")
    );
    assert_eq!(
        fixture
            .store
            .pending_artifact_publish_journals()
            .await
            .expect("pending journals")
            .len(),
        1
    );
    let committed = fixture
        .store
        .mark_artifact_journal_committed("artifact:step:video", "owner-a")
        .await
        .expect("commit journal")
        .expect("commit transition");
    assert_eq!(committed.state, "committed");
    assert!(
        fixture
            .store
            .pending_artifact_publish_journals()
            .await
            .expect("pending journals")
            .is_empty()
    );
}

#[tokio::test]
async fn artifact_operation_key_rejects_different_content() {
    let fixture = RecoveryFixture::create().await;
    fixture
        .store
        .create_or_read_artifact_publish_journal(NewArtifactPublishJournal {
            operation_key: "artifact:step:video",
            run_id: &fixture.run_id,
            run_step_id: &fixture.step_id,
            staged_path: "spool/video.part",
            content_sha256: "sha256:first",
            owner_id: "owner-a",
            expires_after_seconds: 300,
        })
        .await
        .expect("stage journal");
    let err = fixture
        .store
        .create_or_read_artifact_publish_journal(NewArtifactPublishJournal {
            operation_key: "artifact:step:video",
            run_id: &fixture.run_id,
            run_step_id: &fixture.step_id,
            staged_path: "spool/other.part",
            content_sha256: "sha256:other",
            owner_id: "owner-b",
            expires_after_seconds: 300,
        })
        .await
        .expect_err("operation reuse must fail");
    assert!(matches!(
        err,
        StoreError::RecoveryInvariant {
            operation: "create_or_read_artifact_publish_journal",
            ..
        }
    ));
}

#[tokio::test]
async fn cost_operation_key_is_idempotent_and_rejects_changed_amount() {
    let fixture = RecoveryFixture::create().await;
    let input = || NewCostLedger {
        workspace_id: &fixture.workspace_id,
        run_id: Some(&fixture.run_id),
        run_step_id: Some(&fixture.step_id),
        provider: "mock",
        amount: 0.25,
        currency: "USD",
        estimated: true,
    };
    let first = fixture
        .store
        .create_cost_ledger_once("estimate:step", input())
        .await
        .expect("create estimate");
    let replay = fixture
        .store
        .create_cost_ledger_once("estimate:step", input())
        .await
        .expect("replay estimate");
    assert_eq!(first.id, replay.id);
    let err = fixture
        .store
        .create_cost_ledger_once(
            "estimate:step",
            NewCostLedger {
                amount: 0.5,
                ..input()
            },
        )
        .await
        .expect_err("changed amount must fail");
    assert!(matches!(
        err,
        StoreError::RecoveryInvariant {
            operation: "create_cost_ledger_once",
            ..
        }
    ));
}
