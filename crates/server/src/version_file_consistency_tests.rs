use std::collections::BTreeMap;
use std::fs::File;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};

use helixflow_graph::WorkflowGraph;
use helixflow_store::{NewProposal, NewVersion, Store, VersionRecord, VersionSource};

use crate::graph_files::graph_hash;
use crate::version_file_consistency::{
    CandidateCleanupOutcome, CandidateIo, CandidateKind, FsCandidateIo, StrictSha256,
    VersionFileCandidate, VersionFileCandidateSet, VersionFileConsistencyError, read_version_graph,
};

#[test]
fn verified_read_rejects_noncanonical_sha256_text() {
    assert!(StrictSha256::parse("sha256:ABC").is_err());
    assert!(StrictSha256::parse("sha256:abc").is_err());
    assert!(StrictSha256::parse("md5:00000000000000000000000000000000").is_err());
}

#[tokio::test]
async fn verified_read_rejects_missing_version_file() {
    let dir = tempfile::tempdir().expect("temp dir");
    let version = version_record("graphs/missing.json", &graph_hash(b"missing"));

    let error = read_version_graph(dir.path(), &version)
        .await
        .expect_err("missing file must fail closed");

    assert!(matches!(
        error,
        VersionFileConsistencyError::MissingFile { .. }
    ));
}

#[tokio::test]
async fn verified_read_rejects_unsafe_relative_path() {
    let dir = tempfile::tempdir().expect("temp dir");
    let version = version_record("../outside.json", &graph_hash(b"outside"));

    let error = read_version_graph(dir.path(), &version)
        .await
        .expect_err("unsafe path must fail closed");

    assert!(matches!(
        error,
        VersionFileConsistencyError::UnsafeRelativePath { .. }
    ));
}

#[tokio::test]
async fn verified_read_rejects_invalid_hash_before_consuming_file() {
    let dir = tempfile::tempdir().expect("temp dir");
    write_relative(dir.path(), "graphs/graph.json", b"{}").await;
    let version = version_record(
        "graphs/graph.json",
        "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    );

    let error = read_version_graph(dir.path(), &version)
        .await
        .expect_err("uppercase hash must fail closed");

    assert!(matches!(
        error,
        VersionFileConsistencyError::InvalidStoredHash { .. }
    ));
}

#[tokio::test]
async fn verified_read_rejects_hash_mismatch() {
    let dir = tempfile::tempdir().expect("temp dir");
    let bytes = serde_json::to_vec(&sample_graph()).expect("graph bytes");
    write_relative(dir.path(), "graphs/graph.json", &bytes).await;
    let version = version_record("graphs/graph.json", &graph_hash(b"different"));

    let error = read_version_graph(dir.path(), &version)
        .await
        .expect_err("hash mismatch must fail closed");

    assert!(matches!(
        error,
        VersionFileConsistencyError::HashMismatch { .. }
    ));
}

#[tokio::test]
async fn verified_read_rejects_invalid_json_after_hash_validation() {
    let dir = tempfile::tempdir().expect("temp dir");
    let bytes = br#"{"schema_version":"#;
    write_relative(dir.path(), "graphs/graph.json", bytes).await;
    let version = version_record("graphs/graph.json", &graph_hash(bytes));

    let error = read_version_graph(dir.path(), &version)
        .await
        .expect_err("invalid JSON must fail closed");

    assert!(matches!(
        error,
        VersionFileConsistencyError::InvalidJson { .. }
    ));
}

#[tokio::test]
async fn verified_read_accepts_valid_legacy_relative_path() {
    let dir = tempfile::tempdir().expect("temp dir");
    let graph = sample_graph();
    let bytes = serde_json::to_vec_pretty(&graph).expect("legacy graph bytes");
    write_relative(dir.path(), "graphs/legacy-name.json", &bytes).await;
    let version = version_record("graphs/legacy-name.json", &graph_hash(&bytes));

    let actual = read_version_graph(dir.path(), &version)
        .await
        .expect("valid legacy graph");

    assert_eq!(actual, graph);
}

fn version_record(path: &str, hash: &str) -> VersionRecord {
    VersionRecord {
        id: "ver_test".to_owned(),
        workspace_id: "ws_test".to_owned(),
        idx: 1,
        label: "Test graph".to_owned(),
        source: "manual".to_owned(),
        graph_path: path.to_owned(),
        graph_hash: hash.to_owned(),
        parent_id: None,
        semantics_json: None,
        created_at: "2026-07-16T00:00:00Z".to_owned(),
    }
}

async fn write_relative(root: &std::path::Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    tokio::fs::create_dir_all(path.parent().expect("parent"))
        .await
        .expect("create parent");
    tokio::fs::write(path, bytes).await.expect("write file");
}

fn sample_graph() -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::new(),
        edges: Vec::new(),
    }
}

#[test]
fn candidate_publishes_canonical_graph_with_hash_and_no_temp() {
    let dir = tempfile::tempdir().expect("temp dir");
    let graph = sample_graph();
    let mut candidate =
        VersionFileCandidate::from_graph("ws_candidate", CandidateKind::Layout, &graph)
            .expect("candidate");
    let relative_path = candidate.relative_path().to_path_buf();
    let expected_bytes = serde_json::to_vec(&graph).expect("canonical graph bytes");

    candidate.publish(dir.path()).expect("publish candidate");

    assert_eq!(
        std::fs::read(dir.path().join(relative_path)).expect("read candidate"),
        expected_bytes
    );
    assert_eq!(candidate.graph_hash(), graph_hash(&expected_bytes));
    assert_eq!(candidate.workspace_id(), "ws_candidate");
    assert_eq!(candidate.kind(), CandidateKind::Layout);
    assert_eq!(graph_directory_entries(dir.path(), "ws_candidate"), 1);
    candidate.mark_committed().expect("mark committed");
}

#[test]
fn candidate_publish_collision_preserves_existing_bytes_and_removes_loser_temp() {
    let dir = tempfile::tempdir().expect("temp dir");
    let final_id = uuid::Uuid::now_v7();
    let mut winner = VersionFileCandidate::from_graph_with_ids(
        "ws_collision",
        CandidateKind::Ops,
        &sample_graph(),
        final_id,
        uuid::Uuid::now_v7(),
    )
    .expect("winner candidate");
    winner.publish(dir.path()).expect("publish winner");
    let winner_path = dir.path().join(winner.relative_path());
    let winner_bytes = std::fs::read(&winner_path).expect("winner bytes");
    let altered = WorkflowGraph {
        schema_version: 2,
        nodes: BTreeMap::new(),
        edges: Vec::new(),
    };
    let mut loser = VersionFileCandidate::from_graph_with_ids(
        "ws_collision",
        CandidateKind::Ops,
        &altered,
        final_id,
        uuid::Uuid::now_v7(),
    )
    .expect("loser candidate");

    let error = loser
        .publish(dir.path())
        .expect_err("exclusive publish must reject collision");

    assert!(matches!(
        error,
        VersionFileConsistencyError::Io {
            operation: "publish_candidate",
            kind: io::ErrorKind::AlreadyExists,
        }
    ));
    assert_eq!(
        std::fs::read(winner_path).expect("winner remains"),
        winner_bytes
    );
    assert_eq!(graph_directory_entries(dir.path(), "ws_collision"), 1);
}

#[test]
fn candidate_faults_before_completed_publish_leave_no_files() {
    for stage in [
        FaultStage::Create,
        FaultStage::Write,
        FaultStage::SyncFile,
        FaultStage::Publish,
        FaultStage::SyncParent,
    ] {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut candidate =
            VersionFileCandidate::from_graph("ws_fault", CandidateKind::Initial, &sample_graph())
                .expect("candidate");

        candidate
            .publish_with_io(dir.path(), &FaultIo(stage))
            .expect_err("injected fault must fail");

        assert_eq!(
            graph_directory_entries(dir.path(), "ws_fault"),
            0,
            "stage {stage:?}"
        );
        assert!(candidate.mark_committed().is_err());
    }
}

#[test]
fn candidate_remove_fault_is_visible_and_preserves_owned_files_for_recovery() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut candidate = VersionFileCandidate::from_graph(
        "ws_remove_fault",
        CandidateKind::ProposalApplied,
        &sample_graph(),
    )
    .expect("candidate");

    let error = candidate
        .publish_with_io(dir.path(), &FaultIo(FaultStage::Remove))
        .expect_err("remove fault must not report publish success");

    assert!(matches!(
        error,
        VersionFileConsistencyError::PrimaryAndCleanup {
            primary_operation: "remove_candidate_temp",
            primary_kind: Some(io::ErrorKind::PermissionDenied),
            cleanup_operation: "remove_owned_candidate",
            cleanup_kind: Some(io::ErrorKind::PermissionDenied),
        }
    ));
    assert_eq!(graph_directory_entries(dir.path(), "ws_remove_fault"), 2);
    assert!(candidate.mark_committed().is_err());
}

#[test]
fn candidate_write_and_cleanup_fault_reports_both_failures() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut candidate =
        VersionFileCandidate::from_graph("ws_combined", CandidateKind::Ops, &sample_graph())
            .expect("candidate");

    let error = candidate
        .publish_with_io(dir.path(), &FaultIo(FaultStage::WriteAndRemove))
        .expect_err("primary and cleanup faults must both be visible");

    assert!(matches!(
        error,
        VersionFileConsistencyError::PrimaryAndCleanup {
            primary_operation: "write_candidate_temp",
            primary_kind: Some(io::ErrorKind::WriteZero),
            cleanup_operation: "remove_owned_candidate",
            cleanup_kind: Some(io::ErrorKind::PermissionDenied),
        }
    ));
    assert_eq!(graph_directory_entries(dir.path(), "ws_combined"), 1);
}

#[cfg(unix)]
#[test]
fn candidate_rejects_root_external_graph_directory_symlink_without_outside_writes() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("root temp dir");
    let outside = tempfile::tempdir().expect("outside temp dir");
    let workspace_parent = root.path().join("workspaces").join("ws_symlink");
    std::fs::create_dir_all(&workspace_parent).expect("workspace parent");
    let canary = outside.path().join("canary");
    std::fs::write(&canary, b"unchanged").expect("write canary");
    symlink(outside.path(), workspace_parent.join("graphs")).expect("create graphs symlink");
    let mut candidate =
        VersionFileCandidate::from_graph("ws_symlink", CandidateKind::Ops, &sample_graph())
            .expect("candidate");

    let error = candidate
        .publish(root.path())
        .expect_err("external graph directory must fail closed");

    assert!(matches!(
        error,
        VersionFileConsistencyError::UnsafeRelativePath { .. }
    ));
    assert_eq!(std::fs::read(&canary).expect("read canary"), b"unchanged");
    assert_eq!(
        std::fs::read_dir(outside.path())
            .expect("outside entries")
            .count(),
        1
    );
}

#[test]
fn candidate_set_cleans_prior_publications_when_later_publish_fails() {
    let dir = tempfile::tempdir().expect("temp dir");
    let final_id = uuid::Uuid::now_v7();
    let first = VersionFileCandidate::from_graph_with_ids(
        "ws_set",
        CandidateKind::ProposalOps,
        &sample_graph(),
        final_id,
        uuid::Uuid::now_v7(),
    )
    .expect("first candidate");
    let second = VersionFileCandidate::from_graph_with_ids(
        "ws_set",
        CandidateKind::ProposalOps,
        &sample_graph(),
        final_id,
        uuid::Uuid::now_v7(),
    )
    .expect("second candidate");
    let mut set = VersionFileCandidateSet::default();
    set.push(first);
    set.push(second);
    assert_eq!(set.candidates().len(), 2);

    set.publish_all(dir.path())
        .expect_err("second publication must collide");

    assert_eq!(graph_directory_entries(dir.path(), "ws_set"), 0);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FaultStage {
    Create,
    Write,
    SyncFile,
    Publish,
    SyncParent,
    Remove,
    WriteAndRemove,
}

struct FaultIo(FaultStage);

impl CandidateIo for FaultIo {
    fn create_new(&self, path: &std::path::Path) -> io::Result<File> {
        if self.0 == FaultStage::Create {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        FsCandidateIo.create_new(path)
    }

    fn write_all(&self, file: &mut File, bytes: &[u8]) -> io::Result<()> {
        if matches!(self.0, FaultStage::Write | FaultStage::WriteAndRemove) {
            return Err(io::Error::from(io::ErrorKind::WriteZero));
        }
        FsCandidateIo.write_all(file, bytes)
    }

    fn sync_file(&self, file: &File) -> io::Result<()> {
        if self.0 == FaultStage::SyncFile {
            return Err(io::Error::from(io::ErrorKind::Other));
        }
        FsCandidateIo.sync_file(file)
    }

    fn publish_no_replace(
        &self,
        temp: &std::path::Path,
        final_path: &std::path::Path,
    ) -> io::Result<()> {
        if self.0 == FaultStage::Publish {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        FsCandidateIo.publish_no_replace(temp, final_path)
    }

    fn sync_parent(&self, parent: &std::path::Path) -> io::Result<()> {
        if self.0 == FaultStage::SyncParent {
            return Err(io::Error::from(io::ErrorKind::Other));
        }
        FsCandidateIo.sync_parent(parent)
    }

    fn remove_file(&self, path: &std::path::Path) -> io::Result<()> {
        if matches!(self.0, FaultStage::Remove | FaultStage::WriteAndRemove) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        FsCandidateIo.remove_file(path)
    }
}

fn graph_directory_entries(root: &std::path::Path, workspace_id: &str) -> usize {
    let directory = root.join("workspaces").join(workspace_id).join("graphs");
    match std::fs::read_dir(directory) {
        Ok(entries) => entries.count(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => panic!("read graph directory: {error}"),
    }
}

#[tokio::test]
async fn cleanup_removes_exact_unreferenced_candidate() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (store, _db_dir) = open_store().await;
    let mut candidate =
        VersionFileCandidate::from_graph("ws_cleanup", CandidateKind::Ops, &sample_graph())
            .expect("candidate");
    candidate.publish(dir.path()).expect("publish candidate");
    let final_path = dir.path().join(candidate.relative_path());
    assert!(final_path.exists());

    let outcome = candidate
        .cleanup_after_store_error(&store)
        .await
        .expect("cleanup unreferenced candidate");

    assert_eq!(outcome, CandidateCleanupOutcome::Removed);
    assert!(!final_path.exists());
}

#[tokio::test]
async fn cleanup_commit_then_error_preserves_exact_version_reference() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (store, _db_dir) = open_store().await;
    let workspace = store
        .create_workspace("Commit outcome")
        .await
        .expect("create workspace");
    let mut candidate =
        VersionFileCandidate::from_graph(&workspace.id, CandidateKind::Layout, &sample_graph())
            .expect("candidate");
    candidate.publish(dir.path()).expect("publish candidate");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Committed before synthetic error",
            source: VersionSource::Manual,
            graph_path: candidate.relative_path_text().expect("candidate path"),
            graph_hash: candidate.graph_hash(),
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("commit version");
    let final_path = dir.path().join(candidate.relative_path());

    let outcome = candidate
        .cleanup_after_store_error(&store)
        .await
        .expect("reference lookup");

    assert_eq!(outcome, CandidateCleanupOutcome::PreservedReferenced);
    assert!(final_path.exists());
    assert_eq!(
        store
            .version_file_references(candidate.relative_path_text().expect("candidate path"))
            .await
            .expect("version refs")[0]
            .id,
        version.id
    );
}

#[tokio::test]
async fn cleanup_preserves_exact_proposal_reference() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (store, _db_dir) = open_store().await;
    let workspace = store
        .create_workspace("Proposal reference")
        .await
        .expect("create workspace");
    let base = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Base",
            source: VersionSource::Manual,
            graph_path: "legacy/base.json",
            graph_hash: &graph_hash(b"base"),
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("create base");
    let mut candidate = VersionFileCandidate::from_json(
        &workspace.id,
        CandidateKind::ProposalPreview,
        &serde_json::json!({ "preview": true }),
    )
    .expect("candidate");
    candidate.publish(dir.path()).expect("publish candidate");
    let proposal = store
        .create_proposal(NewProposal {
            workspace_id: &workspace.id,
            base_version_id: &base.id,
            kind: "modify",
            title: "Proposal",
            summary: "Proposal",
            ops_path: "legacy/ops.json",
            preview_graph_path: Some(candidate.relative_path_text().expect("candidate path")),
            message_id: None,
        })
        .await
        .expect("create proposal");
    let final_path = dir.path().join(candidate.relative_path());

    let outcome = candidate
        .cleanup_after_store_error(&store)
        .await
        .expect("reference lookup");

    assert_eq!(outcome, CandidateCleanupOutcome::PreservedReferenced);
    assert!(final_path.exists());
    assert_eq!(
        store
            .proposal_file_references(candidate.relative_path_text().expect("candidate path"))
            .await
            .expect("proposal refs")[0]
            .proposal_id,
        proposal.id
    );
}

#[tokio::test]
async fn cleanup_query_uncertainty_preserves_candidate_and_reports_deferred() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (store, _db_dir) = open_store().await;
    let mut candidate =
        VersionFileCandidate::from_graph("ws_query", CandidateKind::Ops, &sample_graph())
            .expect("candidate");
    candidate.publish(dir.path()).expect("publish candidate");
    let final_path = dir.path().join(candidate.relative_path());
    store.pool().close().await;

    let error = candidate
        .cleanup_after_store_error(&store)
        .await
        .expect_err("closed pool makes reference state unknown");

    assert!(matches!(
        error,
        VersionFileConsistencyError::CleanupDeferred {
            operation: "query_version_references",
            kind: None,
        }
    ));
    assert!(final_path.exists());
}

#[tokio::test]
async fn cleanup_remove_failure_preserves_candidate_and_reports_deferred() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (store, _db_dir) = open_store().await;
    let mut candidate =
        VersionFileCandidate::from_graph("ws_cleanup_fault", CandidateKind::Ops, &sample_graph())
            .expect("candidate");
    candidate.publish(dir.path()).expect("publish candidate");
    let final_path = dir.path().join(candidate.relative_path());

    let error = candidate
        .cleanup_after_store_error_with_io(&store, &FaultIo(FaultStage::Remove))
        .await
        .expect_err("remove fault must be visible");

    assert!(matches!(
        error,
        VersionFileConsistencyError::CleanupDeferred {
            operation: "remove_owned_candidate",
            kind: Some(io::ErrorKind::PermissionDenied),
        }
    ));
    assert!(final_path.exists());
}

#[tokio::test]
async fn cleanup_candidate_set_removes_each_unreferenced_publication() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (store, _db_dir) = open_store().await;
    let mut set = VersionFileCandidateSet::default();
    set.push(
        VersionFileCandidate::from_graph(
            "ws_set_cleanup",
            CandidateKind::ProposalOps,
            &sample_graph(),
        )
        .expect("ops candidate"),
    );
    set.push(
        VersionFileCandidate::from_graph(
            "ws_set_cleanup",
            CandidateKind::ProposalPreview,
            &sample_graph(),
        )
        .expect("preview candidate"),
    );
    set.publish_all(dir.path()).expect("publish set");
    assert_eq!(graph_directory_entries(dir.path(), "ws_set_cleanup"), 2);

    let outcomes = set
        .cleanup_after_store_error(&store)
        .await
        .expect("cleanup set");

    assert_eq!(
        outcomes,
        vec![
            CandidateCleanupOutcome::Removed,
            CandidateCleanupOutcome::Removed
        ]
    );
    assert_eq!(graph_directory_entries(dir.path(), "ws_set_cleanup"), 0);
}

#[tokio::test]
async fn cleanup_candidate_set_continues_and_preserves_references_after_earlier_failure() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (store, _db_dir) = open_store().await;
    let workspace = store
        .create_workspace("Partial candidate cleanup")
        .await
        .expect("create workspace");
    let mut set = VersionFileCandidateSet::default();
    set.push(
        VersionFileCandidate::from_graph(
            &workspace.id,
            CandidateKind::ProposalOps,
            &sample_graph(),
        )
        .expect("first candidate"),
    );
    set.push(
        VersionFileCandidate::from_graph(
            &workspace.id,
            CandidateKind::ProposalPreview,
            &sample_graph(),
        )
        .expect("later candidate"),
    );
    set.push(
        VersionFileCandidate::from_graph(&workspace.id, CandidateKind::Layout, &sample_graph())
            .expect("referenced candidate"),
    );
    set.publish_all(dir.path()).expect("publish set");
    let paths: Vec<_> = set
        .candidates()
        .iter()
        .map(|candidate| dir.path().join(candidate.relative_path()))
        .collect();
    store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Referenced candidate",
            source: VersionSource::Manual,
            graph_path: set.candidates()[2]
                .relative_path_text()
                .expect("referenced path"),
            graph_hash: set.candidates()[2].graph_hash(),
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("create exact version reference");
    let io = FailFirstRemoveIo::default();

    let error = set
        .cleanup_after_store_error_with_io(&store, &io)
        .await
        .expect_err("first removal failure must be aggregated");

    assert!(matches!(
        error,
        VersionFileConsistencyError::CleanupBatchDeferred {
            failure_count: 1,
            first_operation: "remove_owned_candidate",
            first_kind: Some(io::ErrorKind::PermissionDenied),
        }
    ));
    assert!(paths[0].exists(), "failed candidate remains owned");
    assert!(
        !paths[1].exists(),
        "later unreferenced candidate was still inspected and removed"
    );
    assert!(
        paths[2].exists(),
        "later referenced candidate was still inspected and preserved"
    );
    assert_eq!(io.remove_calls.load(Ordering::SeqCst), 2);
}

#[test]
fn candidate_set_marks_every_published_candidate_committed() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut set = VersionFileCandidateSet::default();
    set.push(
        VersionFileCandidate::from_graph(
            "ws_set_commit",
            CandidateKind::ProposalOps,
            &sample_graph(),
        )
        .expect("ops candidate"),
    );
    set.push(
        VersionFileCandidate::from_graph(
            "ws_set_commit",
            CandidateKind::ProposalPreview,
            &sample_graph(),
        )
        .expect("preview candidate"),
    );

    set.publish_all(dir.path()).expect("publish set");
    set.mark_all_committed().expect("mark set committed");
}

#[derive(Default)]
struct FailFirstRemoveIo {
    remove_calls: AtomicUsize,
}

impl CandidateIo for FailFirstRemoveIo {
    fn create_new(&self, path: &std::path::Path) -> io::Result<File> {
        FsCandidateIo.create_new(path)
    }

    fn write_all(&self, file: &mut File, bytes: &[u8]) -> io::Result<()> {
        FsCandidateIo.write_all(file, bytes)
    }

    fn sync_file(&self, file: &File) -> io::Result<()> {
        FsCandidateIo.sync_file(file)
    }

    fn publish_no_replace(
        &self,
        temp: &std::path::Path,
        final_path: &std::path::Path,
    ) -> io::Result<()> {
        FsCandidateIo.publish_no_replace(temp, final_path)
    }

    fn sync_parent(&self, parent: &std::path::Path) -> io::Result<()> {
        FsCandidateIo.sync_parent(parent)
    }

    fn remove_file(&self, path: &std::path::Path) -> io::Result<()> {
        if self.remove_calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        FsCandidateIo.remove_file(path)
    }
}

async fn open_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("database temp dir");
    let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    (store, dir)
}
