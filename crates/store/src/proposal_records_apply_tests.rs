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

    let child = store
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
        })
        .await
        .expect("apply proposal");
    let updated = store.proposal(&proposal.id).await.expect("proposal");

    assert_eq!(child.parent_id.as_deref(), Some(base.id.as_str()));
    assert_eq!(updated.state, "applied");
    assert_eq!(
        updated.result_version_id.as_deref(),
        Some(child.id.as_str())
    );
}

#[tokio::test]
async fn applying_proposal_with_message_commits_all_records_together() {
    let (store, workspace_id, base_id, proposal_id, _dir) = apply_fixture().await;

    let result = apply_with_message(&store, &workspace_id, &base_id, &proposal_id)
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

    let error = apply_with_message(&store, &workspace_id, &base_id, &proposal_id)
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

async fn apply_with_message(
    store: &Store,
    workspace_id: &str,
    base_id: &str,
    proposal_id: &str,
) -> StoreResult<ApplyProposalVersionResult> {
    store
        .create_version_after_applying_proposal_with_message(
            ApplyProposalVersionWithMessageRecord {
                proposal_id,
                expected_current_version_id: base_id,
                version: NewVersion {
                    workspace_id,
                    label: "Applied atomically",
                    source: VersionSource::Proposal,
                    graph_path: "graphs/applied-atomic.json",
                    graph_hash: "sha256:applied-atomic",
                    parent_id: Some(base_id),
                },
                message_text: "Applied atomically",
            },
        )
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
