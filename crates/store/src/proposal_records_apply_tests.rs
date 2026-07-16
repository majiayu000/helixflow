use super::*;
use crate::{Store, VersionSource};

async fn open_temp_store() -> (Store, tempfile::TempDir) {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    let dir = tempfile::tempdir().expect("create temp dir");
    let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
    let options = SqliteConnectOptions::from_str(&database_url)
        .expect("parse database URL")
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("connect test pool");
    let store = Store { pool };
    store.run_migrations().await.expect("run migrations");
    (store, dir)
}

#[tokio::test]
async fn creates_and_lists_pending_proposals() {
    let (store, _dir) = open_temp_store().await;
    let workspace = store
        .create_workspace("Proposal workspace")
        .await
        .expect("create workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Base graph",
            source: VersionSource::Manual,
            graph_path: "graphs/base.json",
            graph_hash: "sha256:base",
            parent_id: None,
        })
        .await
        .expect("create version");

    let proposal = store
        .create_proposal(NewProposal {
            workspace_id: &workspace.id,
            base_version_id: &version.id,
            kind: "modify",
            title: "Resize",
            summary: "Change resolution",
            ops_path: "proposals/resize/ops.json",
            preview_graph_path: Some("proposals/resize/preview.json"),
            message_id: None,
        })
        .await
        .expect("create proposal");

    let pending = store
        .latest_pending_proposal(&workspace.id)
        .await
        .expect("pending proposal")
        .expect("pending");
    assert_eq!(pending.id, proposal.id);
    assert_eq!(pending.state, "pending");
}

#[tokio::test]
async fn resolving_proposal_rejects_repeated_resolution() {
    let (store, _dir) = open_temp_store().await;
    let workspace = store
        .create_workspace("Proposal workspace")
        .await
        .expect("create workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Base graph",
            source: VersionSource::Manual,
            graph_path: "graphs/base.json",
            graph_hash: "sha256:base",
            parent_id: None,
        })
        .await
        .expect("create version");
    let proposal = store
        .create_proposal(NewProposal {
            workspace_id: &workspace.id,
            base_version_id: &version.id,
            kind: "modify",
            title: "Resize",
            summary: "Change resolution",
            ops_path: "proposals/resize/ops.json",
            preview_graph_path: None,
            message_id: None,
        })
        .await
        .expect("create proposal");

    let dismissed = store
        .resolve_proposal(ResolveProposal {
            proposal_id: &proposal.id,
            workspace_id: &workspace.id,
            state: ProposalResolutionState::Dismissed,
            result_version_id: None,
        })
        .await
        .expect("dismiss proposal");
    assert_eq!(dismissed.state, "dismissed");

    assert!(matches!(
        store
            .resolve_proposal(ResolveProposal {
                proposal_id: &proposal.id,
                workspace_id: &workspace.id,
                state: ProposalResolutionState::Dismissed,
                result_version_id: None,
            })
            .await,
        Err(StoreError::ProposalStateConflict { .. })
    ));
}

#[tokio::test]
async fn applying_proposal_creates_version_and_marks_applied() {
    let (store, _dir) = open_temp_store().await;
    let workspace = store
        .create_workspace("Proposal workspace")
        .await
        .expect("create workspace");
    let base = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Base graph",
            source: VersionSource::Manual,
            graph_path: "graphs/base.json",
            graph_hash: "sha256:base",
            parent_id: None,
        })
        .await
        .expect("create version");
    let proposal = store
        .create_proposal(NewProposal {
            workspace_id: &workspace.id,
            base_version_id: &base.id,
            kind: "modify",
            title: "Resize",
            summary: "Change resolution",
            ops_path: "proposals/resize/ops.json",
            preview_graph_path: None,
            message_id: None,
        })
        .await
        .expect("create proposal");

    let result = store
        .create_version_after_applying_proposal(ApplyProposalVersionRecord {
            proposal_id: &proposal.id,
            expected_current_version_id: &base.id,
            version: NewVersion {
                workspace_id: &workspace.id,
                label: "Applied proposal",
                source: VersionSource::Proposal,
                graph_path: "graphs/applied.json",
                graph_hash: "sha256:applied",
                parent_id: Some(&base.id),
            },
            message_text: "Applied proposal",
        })
        .await
        .expect("apply proposal");
    let updated = store.proposal(&proposal.id).await.expect("proposal");

    assert_eq!(result.version.parent_id.as_deref(), Some(base.id.as_str()));
    assert_eq!(updated.state, "applied");
    assert_eq!(
        updated.result_version_id.as_deref(),
        Some(result.version.id.as_str())
    );
}

#[tokio::test]
async fn applying_proposal_with_message_commits_all_records_together() {
    let (store, workspace_id, base_id, proposal_id, _dir) = apply_fixture().await;

    let result = apply_proposal(
        &store,
        &workspace_id,
        &base_id,
        &proposal_id,
        Some(base_id.as_str()),
    )
    .await
    .expect("apply proposal with message");
    let proposal = store.proposal(&proposal_id).await.expect("proposal");
    let workspace = store.workspace(&workspace_id).await.expect("workspace");

    assert_eq!(proposal.state, "applied");
    assert_eq!(
        proposal.result_version_id.as_deref(),
        Some(result.version.id.as_str())
    );
    assert_eq!(
        workspace.cur_version_id.as_deref(),
        Some(result.version.id.as_str())
    );
    assert_eq!(result.message.kind, "proposal_applied");
    assert_eq!(result.message.role, "agent");
    assert_eq!(result.message.text.as_deref(), Some("Applied atomically"));
    assert_eq!(result.message.ref_id.as_deref(), Some(proposal_id.as_str()));
    assert_eq!(
        result.message.attachment_ids_json.as_deref(),
        Some(format!(r#"{{"versionId":"{}"}}"#, result.version.id).as_str())
    );
}

#[tokio::test]
async fn applying_proposal_rejects_missing_parent_before_writes() {
    assert_manual_invariant_is_write_free(None).await;
}

#[tokio::test]
async fn applying_proposal_rejects_unrelated_parent_before_writes() {
    assert_manual_invariant_is_write_free(Some("ver_unrelated")).await;
}

#[tokio::test]
async fn applying_proposal_rejects_proposal_base_mismatch_before_writes() {
    let (store, workspace_id, base_id, proposal_id, _dir) = apply_fixture().await;
    let newer = store
        .create_version_after(
            NewVersion {
                workspace_id: &workspace_id,
                label: "New current",
                source: VersionSource::Manual,
                graph_path: "graphs/new-current.json",
                graph_hash: "sha256:new-current",
                parent_id: Some(&base_id),
            },
            &base_id,
        )
        .await
        .expect("advance current");

    let error = apply_proposal(
        &store,
        &workspace_id,
        &newer.id,
        &proposal_id,
        Some(&newer.id),
    )
    .await
    .expect_err("proposal base must equal expected current");

    assert_eq!(
        error.to_string(),
        format!(
            "proposal `{proposal_id}` expected base version `{}` but found `{base_id}`",
            newer.id
        )
    );
    assert!(matches!(
        error,
        StoreError::ProposalBaseMismatch {
            proposal_id: actual_proposal_id,
            expected_base_version_id,
            actual_base_version_id,
        } if actual_proposal_id == proposal_id
            && expected_base_version_id == newer.id
            && actual_base_version_id == base_id
    ));
    assert_manual_state(&store, &workspace_id, &newer.id, &proposal_id, 2, "pending").await;
}

#[tokio::test]
async fn applying_proposal_rejects_stale_current_without_partial_writes() {
    let (store, workspace_id, base_id, proposal_id, _dir) = apply_fixture().await;
    let newer = store
        .create_version_after(
            NewVersion {
                workspace_id: &workspace_id,
                label: "Newer",
                source: VersionSource::Manual,
                graph_path: "graphs/newer.json",
                graph_hash: "sha256:newer",
                parent_id: Some(&base_id),
            },
            &base_id,
        )
        .await
        .expect("advance current");

    let error = apply_proposal(
        &store,
        &workspace_id,
        &base_id,
        &proposal_id,
        Some(&base_id),
    )
    .await
    .expect_err("stale current must conflict");

    assert!(matches!(
        error,
        StoreError::VersionConflict {
            expected_version_id,
            actual_version_id: Some(actual_version_id),
            ..
        } if expected_version_id == base_id && actual_version_id == newer.id
    ));
    assert_manual_state(&store, &workspace_id, &newer.id, &proposal_id, 2, "pending").await;
}

#[tokio::test]
async fn applying_proposal_rejects_non_pending_state_without_partial_writes() {
    let (store, workspace_id, base_id, proposal_id, _dir) = apply_fixture().await;
    store
        .resolve_proposal(ResolveProposal {
            proposal_id: &proposal_id,
            workspace_id: &workspace_id,
            state: ProposalResolutionState::Dismissed,
            result_version_id: None,
        })
        .await
        .expect("dismiss proposal");

    let error = apply_proposal(
        &store,
        &workspace_id,
        &base_id,
        &proposal_id,
        Some(&base_id),
    )
    .await
    .expect_err("resolved proposal cannot be applied");

    assert!(matches!(
        error,
        StoreError::ProposalStateConflict {
            actual_state: Some(actual_state),
            ..
        } if actual_state == "dismissed"
    ));
    assert_manual_state(
        &store,
        &workspace_id,
        &base_id,
        &proposal_id,
        1,
        "dismissed",
    )
    .await;
}

#[tokio::test]
async fn suppressed_proposal_update_is_classified_and_rolls_back() {
    let (store, workspace_id, base_id, proposal_id, _dir) = apply_fixture().await;
    sqlx::query(
        r#"
        CREATE TEMP TRIGGER ignore_applied_proposal_update
        BEFORE UPDATE OF state ON proposals
        WHEN NEW.state = 'applied'
        BEGIN
          SELECT RAISE(IGNORE);
        END
        "#,
    )
    .execute(store.pool())
    .await
    .expect("install ignored-update trigger");

    let error = apply_proposal(
        &store,
        &workspace_id,
        &base_id,
        &proposal_id,
        Some(&base_id),
    )
    .await
    .expect_err("ignored proposal update must be classified");

    assert!(matches!(
        error,
        StoreError::StatementInvariant {
            operation: "mark_applied_proposal",
            expected_rows: 1,
            actual_rows: 0,
        }
    ));
    assert_manual_state(&store, &workspace_id, &base_id, &proposal_id, 1, "pending").await;
}

#[tokio::test]
async fn diverged_proposal_after_suppressed_update_is_state_conflict_and_rolls_back() {
    let (store, workspace_id, base_id, proposal_id, _dir) = apply_fixture().await;
    sqlx::query(
        r#"
        CREATE TEMP TRIGGER diverge_applied_proposal_update
        BEFORE UPDATE OF state ON proposals
        WHEN NEW.state = 'applied'
        BEGIN
          UPDATE proposals SET state = 'dismissed' WHERE id = OLD.id;
          SELECT RAISE(IGNORE);
        END
        "#,
    )
    .execute(store.pool())
    .await
    .expect("install diverging-update trigger");

    let error = apply_proposal(
        &store,
        &workspace_id,
        &base_id,
        &proposal_id,
        Some(&base_id),
    )
    .await
    .expect_err("diverged proposal state must conflict");

    assert!(matches!(
        error,
        StoreError::ProposalStateConflict {
            actual_state: Some(actual_state),
            ..
        } if actual_state == "dismissed"
    ));
    assert_manual_state(&store, &workspace_id, &base_id, &proposal_id, 1, "pending").await;
}

#[tokio::test]
async fn suppressed_current_update_is_classified_and_rolls_back() {
    let (store, workspace_id, base_id, proposal_id, _dir) = apply_fixture().await;
    sqlx::query(
        r#"
        CREATE TEMP TRIGGER ignore_applied_current_update
        BEFORE UPDATE OF cur_version_id ON workspaces
        WHEN NEW.cur_version_id != OLD.cur_version_id
        BEGIN
          SELECT RAISE(IGNORE);
        END
        "#,
    )
    .execute(store.pool())
    .await
    .expect("install ignored-update trigger");

    let error = apply_proposal(
        &store,
        &workspace_id,
        &base_id,
        &proposal_id,
        Some(&base_id),
    )
    .await
    .expect_err("ignored current update must be classified");

    assert!(matches!(
        error,
        StoreError::StatementInvariant {
            operation: "advance_applied_proposal_current",
            expected_rows: 1,
            actual_rows: 0,
        }
    ));
    assert_manual_state(&store, &workspace_id, &base_id, &proposal_id, 1, "pending").await;
}

#[tokio::test]
async fn diverged_current_after_suppressed_update_is_version_conflict_and_rolls_back() {
    let (store, workspace_id, base_id, proposal_id, _dir) = apply_fixture().await;
    sqlx::query(
        r#"
        CREATE TEMP TRIGGER diverge_applied_current_update
        BEFORE UPDATE OF cur_version_id ON workspaces
        WHEN NEW.cur_version_id != OLD.cur_version_id
        BEGIN
          UPDATE workspaces SET cur_version_id = NULL WHERE id = OLD.id;
          SELECT RAISE(IGNORE);
        END
        "#,
    )
    .execute(store.pool())
    .await
    .expect("install diverging-update trigger");

    let error = apply_proposal(
        &store,
        &workspace_id,
        &base_id,
        &proposal_id,
        Some(&base_id),
    )
    .await
    .expect_err("diverged current must conflict");

    assert!(matches!(
        error,
        StoreError::VersionConflict {
            expected_version_id,
            actual_version_id: None,
            ..
        } if expected_version_id == base_id
    ));
    assert_manual_state(&store, &workspace_id, &base_id, &proposal_id, 1, "pending").await;
}

#[tokio::test]
async fn applied_version_insert_failure_rolls_back_every_record() {
    assert_manual_apply_fault_rolls_back(
        r#"
        CREATE TEMP TRIGGER fail_applied_version_insert
        BEFORE INSERT ON versions
        WHEN NEW.label = 'Applied atomically'
        BEGIN
          SELECT RAISE(ABORT, 'fail_applied_version_insert');
        END
        "#,
    )
    .await;
}

#[tokio::test]
async fn applied_current_update_failure_rolls_back_every_record() {
    assert_manual_apply_fault_rolls_back(
        r#"
        CREATE TEMP TRIGGER fail_applied_current_update
        BEFORE UPDATE OF cur_version_id ON workspaces
        WHEN NEW.cur_version_id != OLD.cur_version_id
        BEGIN
          SELECT RAISE(ABORT, 'fail_applied_current_update');
        END
        "#,
    )
    .await;
}

#[tokio::test]
async fn applied_proposal_update_failure_rolls_back_every_record() {
    assert_manual_apply_fault_rolls_back(
        r#"
        CREATE TEMP TRIGGER fail_applied_proposal_update
        BEFORE UPDATE OF state ON proposals
        WHEN NEW.state = 'applied'
        BEGIN
          SELECT RAISE(ABORT, 'fail_applied_proposal_update');
        END
        "#,
    )
    .await;
}

#[tokio::test]
async fn applied_message_insert_failure_rolls_back_every_record() {
    assert_manual_apply_fault_rolls_back(
        r#"
        CREATE TEMP TRIGGER fail_applied_message_insert
        BEFORE INSERT ON messages
        WHEN NEW.kind = 'proposal_applied'
        BEGIN
          SELECT RAISE(ABORT, 'fail_applied_message_insert');
        END
        "#,
    )
    .await;
}

async fn assert_manual_apply_fault_rolls_back(trigger_sql: &str) {
    let (store, workspace_id, base_id, proposal_id, _dir) = apply_fixture().await;
    sqlx::query(trigger_sql)
        .execute(store.pool())
        .await
        .expect("install fault trigger");

    let error = apply_proposal(
        &store,
        &workspace_id,
        &base_id,
        &proposal_id,
        Some(base_id.as_str()),
    )
    .await
    .expect_err("fault must abort proposal transaction");

    assert!(matches!(error, StoreError::Sqlx(_)));
    assert_eq!(
        store
            .workspace(&workspace_id)
            .await
            .expect("workspace")
            .cur_version_id
            .as_deref(),
        Some(base_id.as_str())
    );
    assert_eq!(
        store
            .versions_for_workspace(&workspace_id)
            .await
            .expect("versions")
            .len(),
        1
    );
    assert!(
        store
            .workspace_messages(&workspace_id)
            .await
            .expect("messages")
            .is_empty()
    );
    let proposal = store.proposal(&proposal_id).await.expect("proposal");
    assert_eq!(proposal.state, "pending");
    assert_eq!(proposal.result_version_id, None);
}

async fn assert_manual_invariant_is_write_free(parent_id: Option<&str>) {
    let (store, workspace_id, base_id, proposal_id, _dir) = apply_fixture().await;
    let error = apply_proposal(&store, &workspace_id, &base_id, &proposal_id, parent_id)
        .await
        .expect_err("declared parent must equal expected current");
    assert!(matches!(
        error,
        StoreError::VersionParentMismatch {
            expected_parent_version_id,
            actual_parent_version_id,
            ..
        } if expected_parent_version_id == base_id
            && actual_parent_version_id.as_deref() == parent_id
    ));
    assert_manual_state(&store, &workspace_id, &base_id, &proposal_id, 1, "pending").await;
}

async fn assert_manual_state(
    store: &Store,
    workspace_id: &str,
    current_version_id: &str,
    proposal_id: &str,
    version_count: usize,
    proposal_state: &str,
) {
    assert_eq!(
        store
            .workspace(workspace_id)
            .await
            .expect("workspace state")
            .cur_version_id
            .as_deref(),
        Some(current_version_id)
    );
    assert_eq!(
        store
            .versions_for_workspace(workspace_id)
            .await
            .expect("versions")
            .len(),
        version_count
    );
    assert!(
        store
            .workspace_messages(workspace_id)
            .await
            .expect("messages")
            .is_empty()
    );
    let proposal = store.proposal(proposal_id).await.expect("proposal state");
    assert_eq!(proposal.state, proposal_state);
    assert_eq!(proposal.result_version_id, None);
}

async fn apply_proposal(
    store: &Store,
    workspace_id: &str,
    base_id: &str,
    proposal_id: &str,
    parent_id: Option<&str>,
) -> StoreResult<ApplyProposalVersionResult> {
    store
        .create_version_after_applying_proposal(ApplyProposalVersionRecord {
            proposal_id,
            expected_current_version_id: base_id,
            version: NewVersion {
                workspace_id,
                label: "Applied atomically",
                source: VersionSource::Proposal,
                graph_path: "graphs/applied-atomic.json",
                graph_hash: "sha256:applied-atomic",
                parent_id,
            },
            message_text: "Applied atomically",
        })
        .await
}

async fn apply_fixture() -> (Store, String, String, String, tempfile::TempDir) {
    let (store, dir) = open_temp_store().await;
    let workspace = store
        .create_workspace("Atomic manual proposal")
        .await
        .expect("create workspace");
    let base = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Base",
            source: VersionSource::Manual,
            graph_path: "graphs/base-atomic.json",
            graph_hash: "sha256:base-atomic",
            parent_id: None,
        })
        .await
        .expect("create base");
    let proposal = store
        .create_proposal(NewProposal {
            workspace_id: &workspace.id,
            base_version_id: &base.id,
            kind: "modify",
            title: "Atomic apply",
            summary: "Atomic apply",
            ops_path: "proposals/atomic/ops.json",
            preview_graph_path: Some("proposals/atomic/preview.json"),
            message_id: None,
        })
        .await
        .expect("create proposal");
    (store, workspace.id, base.id, proposal.id, dir)
}
