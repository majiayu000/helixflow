use tempfile::TempDir;

use super::{
    ApplyRunFixVersionRecord, NewProposal, NewRun, NewVersion, RunFixClaim, RunFixSnapshot, Store,
    StoreError, VersionSource,
};

async fn failed_agent_root(store: &Store, label: &str) -> (String, String, String) {
    let workspace = store.create_workspace(label).await.expect("workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label,
            source: VersionSource::Manual,
            graph_path: "graphs/fix-root.json",
            graph_hash: "sha256:fix-root",
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("version");
    let (run, chain) = store
        .create_agent_run_with_repair_chain(NewRun {
            workspace_id: &workspace.id,
            version_id: &version.id,
            group_id: None,
            label,
            trigger: "agent",
            plan_json: Some(r#"{"steps":[]}"#),
            estimate_json: Some(r#"{"amount":0.0,"currency":"USD","unknown":false}"#),
            status: "running",
        })
        .await
        .expect("agent root");
    store
        .request_run_terminalization(&run.id, "failed", Some(r#"{"error":"safe"}"#))
        .await
        .expect("request terminal");
    assert!(
        store
            .claim_run_terminalization(&run.id, "test-settler", 60)
            .await
            .expect("claim terminal")
    );
    store
        .complete_run_terminalization(&run.id, "test-settler")
        .await
        .expect("complete terminal")
        .expect("terminal run");
    store
        .complete_failure_continuation(&run.id, true)
        .await
        .expect("exhaust retry");
    (run.id, chain.id, workspace.id)
}

fn snapshot<'a>() -> RunFixSnapshot<'a> {
    RunFixSnapshot {
        expected_runtime_provider_id: None,
        effective_provider_id: "mock",
        expected_recovery_scope_fingerprint: "sha256:scope",
        provider_catalog_fingerprint: "sha256:catalog",
    }
}

#[tokio::test]
async fn fix_attempt_claims_are_unique_bounded_and_evented() {
    let dir = TempDir::new().expect("temp dir");
    let store = Store::open(&format!(
        "sqlite://{}",
        dir.path().join("fix.sqlite").display()
    ))
    .await
    .expect("open store");
    let (source_run_id, chain_id, _) = failed_agent_root(&store, "bounded fix").await;

    let first = store
        .claim_run_fix_attempt(&source_run_id, 2, snapshot())
        .await
        .expect("claim first");
    let RunFixClaim::Claimed(first) = first else {
        panic!("first attempt must be claimed");
    };
    assert_eq!(first.chain_id, chain_id);
    assert_eq!(first.attempt_index, 1);
    assert_eq!(
        store
            .claim_run_fix_attempt(&source_run_id, 2, snapshot())
            .await
            .expect("duplicate claim"),
        RunFixClaim::NotEligible
    );
    store
        .fail_run_fix_attempt(&first.id, "FIX_AGENT_FAILED", Some("safe failure"))
        .await
        .expect("fail first");

    let second = store
        .claim_run_fix_attempt(&source_run_id, 2, snapshot())
        .await
        .expect("claim second");
    let RunFixClaim::Claimed(second) = second else {
        panic!("second attempt must be claimed");
    };
    assert_eq!(second.attempt_index, 2);
    store
        .fail_run_fix_attempt(&second.id, "FIX_PROPOSAL_INVALID", None)
        .await
        .expect("fail second");
    assert_eq!(
        store
            .failure_continuation(&source_run_id)
            .await
            .expect("continuation")
            .expect("continuation row")
            .state,
        "fix_completed"
    );
    let outbox = store
        .pending_run_fix_outbox()
        .await
        .expect("pending outbox");
    assert_eq!(outbox.len(), 3);
    assert_eq!(outbox[2].event_name, "run.fix_exhausted");
    let event = store
        .persist_run_fix_outbox_event(&outbox[0].id)
        .await
        .expect("persist event")
        .expect("event");
    assert_eq!(event.ev, "run.fix_attempt");
    assert!(
        store
            .mark_run_fix_outbox_broadcasted(&outbox[0].id)
            .await
            .expect("broadcast")
    );
}

#[tokio::test]
async fn zero_fix_limit_exhausts_without_creating_attempt() {
    let dir = TempDir::new().expect("temp dir");
    let store = Store::open(&format!(
        "sqlite://{}",
        dir.path().join("zero.sqlite").display()
    ))
    .await
    .expect("open store");
    let (source_run_id, _, _) = failed_agent_root(&store, "zero fix").await;
    assert_eq!(
        store
            .claim_run_fix_attempt(&source_run_id, 0, snapshot())
            .await
            .expect("claim zero"),
        RunFixClaim::Exhausted
    );
    let outbox = store
        .pending_run_fix_outbox()
        .await
        .expect("pending outbox");
    assert_eq!(outbox.len(), 1);
    assert_eq!(outbox[0].event_name, "run.fix_exhausted");
}

#[tokio::test]
async fn fix_version_apply_atomically_advances_workspace_and_attempt() {
    let dir = TempDir::new().expect("temp dir");
    let store = Store::open(&format!(
        "sqlite://{}",
        dir.path().join("apply.sqlite").display()
    ))
    .await
    .expect("open store");
    let (source_run_id, _, workspace_id) = failed_agent_root(&store, "apply fix").await;
    let source = store.run(&source_run_id).await.expect("source");
    let attempt = claim_running_attempt(&store, &source_run_id).await;
    let result = apply_fix_version(&store, &attempt.id, &workspace_id, &source.version_id)
        .await
        .expect("apply fix");

    assert_eq!(result.attempt.state, "version_applied");
    assert_eq!(
        result.attempt.target_version_id.as_deref(),
        Some(result.applied.version.id.as_str())
    );
    assert_eq!(
        store
            .workspace(&workspace_id)
            .await
            .expect("workspace")
            .cur_version_id
            .as_deref(),
        Some(result.applied.version.id.as_str())
    );
    assert_eq!(
        result.applied.version.parent_id.as_deref(),
        Some(source.version_id.as_str())
    );

    let replay = apply_fix_version(&store, &attempt.id, &workspace_id, &source.version_id)
        .await
        .expect("idempotent replay");
    assert_eq!(replay.applied.version.id, result.applied.version.id);
}

#[tokio::test]
async fn fix_version_apply_rejects_nullable_provider_selection_drift_without_partial_rows() {
    let dir = TempDir::new().expect("temp dir");
    let store = Store::open(&format!(
        "sqlite://{}",
        dir.path().join("apply-conflict.sqlite").display()
    ))
    .await
    .expect("open store");
    let (source_run_id, _, workspace_id) = failed_agent_root(&store, "provider conflict").await;
    let source = store.run(&source_run_id).await.expect("source");
    let attempt = claim_running_attempt(&store, &source_run_id).await;
    store
        .set_workspace_runtime_provider(&workspace_id, Some("mock"))
        .await
        .expect("change provider selector");

    let error = apply_fix_version(&store, &attempt.id, &workspace_id, &source.version_id)
        .await
        .expect_err("provider drift rejects apply");
    assert!(matches!(
        error,
        StoreError::RunFixGuardConflict {
            code: "FIX_PROVIDER_CHANGED"
        }
    ));
    assert_eq!(
        store
            .run_fix_attempt(&attempt.id)
            .await
            .expect("attempt")
            .expect("attempt row")
            .state,
        "agent_running"
    );
    assert_eq!(
        store
            .workspace(&workspace_id)
            .await
            .expect("workspace")
            .cur_version_id
            .as_deref(),
        Some(source.version_id.as_str())
    );
}

async fn claim_running_attempt(store: &Store, source_run_id: &str) -> super::RunFixAttemptRecord {
    let RunFixClaim::Claimed(attempt) = store
        .claim_run_fix_attempt(source_run_id, 1, snapshot())
        .await
        .expect("claim attempt")
    else {
        panic!("attempt must be claimed");
    };
    store
        .mark_run_fix_agent_running(&attempt.id)
        .await
        .expect("agent running")
        .expect("attempt")
}

async fn apply_fix_version(
    store: &Store,
    attempt_id: &str,
    workspace_id: &str,
    source_version_id: &str,
) -> super::StoreResult<super::ApplyRunFixVersionResult> {
    store
        .apply_run_fix_version(ApplyRunFixVersionRecord {
            attempt_id,
            proposal: NewProposal {
                workspace_id,
                base_version_id: source_version_id,
                kind: "fix",
                title: "safe fix",
                summary: "safe summary",
                ops_path: "graphs/fix-ops.json",
                preview_graph_path: Some("graphs/fix-preview.json"),
                message_id: None,
            },
            version: NewVersion {
                workspace_id,
                label: "Agent fix",
                source: VersionSource::Proposal,
                graph_path: "graphs/fix-applied.json",
                graph_hash: "sha256:fix-applied",
                parent_id: Some(source_version_id),
                semantics_json: None,
            },
            message_text: "已应用安全修复。",
            actual_effective_provider_id: "mock",
            actual_recovery_scope_fingerprint: "sha256:scope",
            actual_provider_catalog_fingerprint: "sha256:catalog",
        })
        .await
}
