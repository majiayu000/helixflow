use std::collections::BTreeMap;
use std::path::Path;

use helixflow_graph::WorkflowGraph;
use helixflow_run::EventBus;
use helixflow_store::{NewProposal, NewRun, NewVersion, Store, VersionSource};

use crate::app_state::{AppState, AppStateError};
use crate::graph_files::graph_hash;
use crate::version_file_reconciliation::{
    ReconciliationIo, ReconciliationReport, VersionFileReconciliationError,
    reconcile_version_files, reconcile_version_files_with_io,
};
use crate::version_file_reconciliation_event;

#[tokio::test]
async fn startup_verifies_references_and_removes_only_owned_orphans() {
    let fixture = Fixture::new().await;
    let (workspace_id, _version_id) = fixture.seed_valid_version().await;
    fixture.seed_valid_proposal(&workspace_id).await;
    let orphan = format!(
        "workspaces/{workspace_id}/graphs/ops-{}.json",
        uuid::Uuid::now_v7().simple()
    );
    let orphan_temp = format!(
        "workspaces/{workspace_id}/graphs/.hf-layout-{}-{}.tmp",
        uuid::Uuid::now_v7().simple(),
        uuid::Uuid::now_v7().simple()
    );
    let migration_orphan = format!(
        "workspaces/{workspace_id}/graphs/migration-{}.json",
        uuid::Uuid::now_v7().simple()
    );
    let migration_temp = format!(
        "workspaces/{workspace_id}/graphs/.hf-migration-{}-{}.tmp",
        uuid::Uuid::now_v7().simple(),
        uuid::Uuid::now_v7().simple()
    );
    let legacy = format!("workspaces/{workspace_id}/graphs/legacy-graph.json");
    let keyed = format!(
        "workspaces/{workspace_id}/graphs/ops-key-{}.json",
        "a".repeat(64)
    );
    fixture.write(&orphan, b"orphan").await;
    fixture.write(&orphan_temp, b"temp").await;
    fixture.write(&migration_orphan, b"orphan").await;
    fixture.write(&migration_temp, b"temp").await;
    fixture.write(&legacy, b"legacy").await;
    fixture.write(&keyed, b"keyed").await;

    let report = reconcile_version_files(&fixture.store, fixture.root())
        .await
        .expect("reconcile valid startup state");

    assert_eq!(report.verified_versions, 1);
    assert_eq!(report.verified_proposal_files, 2);
    assert_eq!(report.removed_orphans, 4);
    assert_eq!(report.retained_unknown, 2);
    assert_eq!(report.corrupt_references, 0);
    assert_eq!(report.path_categories["referenced_candidate"], 3);
    assert!(!fixture.root().join(orphan).exists());
    assert!(!fixture.root().join(orphan_temp).exists());
    assert!(!fixture.root().join(migration_orphan).exists());
    assert!(!fixture.root().join(migration_temp).exists());
    assert!(fixture.root().join(legacy).exists());
    assert!(fixture.root().join(keyed).exists());
    let versions = fixture
        .store
        .versions_for_workspace(&workspace_id)
        .await
        .expect("versions");
    let proposals = fixture
        .store
        .workspace_proposals(&workspace_id)
        .await
        .expect("proposals");
    assert!(fixture.root().join(&versions[0].graph_path).exists());
    assert!(fixture.root().join(&proposals[0].ops_path).exists());
    assert!(
        fixture
            .root()
            .join(
                proposals[0]
                    .preview_graph_path
                    .as_deref()
                    .expect("preview path"),
            )
            .exists()
    );
}

#[tokio::test]
async fn startup_retains_physical_targets_referenced_through_safe_aliases() {
    let fixture = Fixture::new().await;
    let workspace = fixture
        .store
        .create_workspace("safe aliases")
        .await
        .expect("workspace");
    let graph = sample_graph();
    let graph_bytes = serde_json::to_vec(&graph).expect("graph JSON");
    let graph_relative = format!(
        "workspaces/{}/graphs/initial-{}.json",
        workspace.id,
        uuid::Uuid::now_v7().simple()
    );
    fixture.write(&graph_relative, &graph_bytes).await;
    let graph_alias = format!("./{graph_relative}");
    let version = fixture
        .store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "aliased graph",
            source: VersionSource::Manual,
            graph_path: &graph_alias,
            graph_hash: &graph_hash(&graph_bytes),
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("version");
    let ops_relative = format!(
        "workspaces/{}/graphs/proposal-ops-{}.json",
        workspace.id,
        uuid::Uuid::now_v7().simple()
    );
    fixture.write(&ops_relative, b"[]").await;
    let ops_alias = ops_relative.replacen("/graphs/", "//graphs/", 1);
    fixture
        .store
        .create_proposal(NewProposal {
            workspace_id: &workspace.id,
            base_version_id: &version.id,
            kind: "modify",
            title: "aliased proposal",
            summary: "aliased proposal",
            ops_path: &ops_alias,
            preview_graph_path: None,
            message_id: None,
        })
        .await
        .expect("proposal");

    let report = reconcile_version_files(&fixture.store, fixture.root())
        .await
        .expect("safe aliases remain compatible");

    assert!(fixture.root().join(graph_relative).exists());
    assert!(fixture.root().join(ops_relative).exists());
    assert_eq!(report.removed_orphans, 0);
    assert_eq!(report.path_categories["aliased_reference_candidate"], 2);
}

#[tokio::test]
async fn startup_retains_non_v7_and_uppercase_uuid_names_as_unknown() {
    let fixture = Fixture::new().await;
    let workspace = fixture
        .store
        .create_workspace("strict UUID ownership")
        .await
        .expect("workspace");
    let lowercase_v4 = format!(
        "workspaces/{}/graphs/ops-550e8400e29b41d4a716446655440000.json",
        workspace.id
    );
    let uppercase_v7 = format!(
        "workspaces/{}/graphs/layout-{}.json",
        workspace.id,
        uuid::Uuid::now_v7().simple().to_string().to_uppercase()
    );
    let nil_uuid = format!(
        "workspaces/{}/graphs/initial-00000000000000000000000000000000.json",
        workspace.id
    );
    fixture.write(&lowercase_v4, b"v4").await;
    fixture.write(&uppercase_v7, b"uppercase v7").await;
    fixture.write(&nil_uuid, b"nil").await;

    let report = reconcile_version_files(&fixture.store, fixture.root())
        .await
        .expect("non-owned UUID names are legacy unknowns");

    assert!(fixture.root().join(lowercase_v4).exists());
    assert!(fixture.root().join(uppercase_v7).exists());
    assert!(fixture.root().join(nil_uuid).exists());
    assert_eq!(report.removed_orphans, 0);
    assert_eq!(report.retained_unknown, 3);
    assert_eq!(report.path_categories["unknown_graph_file"], 3);
}

#[tokio::test]
async fn startup_open_retains_success_report_in_app_state() {
    let fixture = Fixture::new().await;
    let (workspace_id, _version_id) = fixture.seed_valid_version().await;
    fixture.seed_valid_proposal(&workspace_id).await;

    let state = AppState::open_for_test(EventBus::new(16), fixture.root().to_path_buf())
        .await
        .expect("open app state through production startup gate");
    let retained = state.reconciliation_report.clone();

    assert_eq!(retained.verified_versions, 1);
    assert_eq!(retained.verified_proposal_files, 2);
    assert_eq!(retained.removed_orphans, 0);
    assert_eq!(retained.path_categories["version_graph"], 1);
    assert_eq!(retained.path_categories["proposal_ops"], 1);
    assert_eq!(retained.path_categories["proposal_preview"], 1);
    assert_eq!(retained.path_categories["referenced_candidate"], 3);
}

#[test]
fn startup_event_is_single_line_structured_and_redacted() {
    let report = ReconciliationReport {
        verified_versions: 2,
        verified_proposal_files: 3,
        removed_orphans: 1,
        retained_unknown: 4,
        corrupt_references: 0,
        path_categories: BTreeMap::from([("version_graph".to_owned(), 2)]),
    };

    let event = version_file_reconciliation_event(&report);
    let value: serde_json::Value = serde_json::from_str(&event).expect("event JSON");

    assert!(!event.contains('\n'));
    assert_eq!(value["event"], "version_file_reconciliation");
    assert_eq!(value["report"]["verified_versions"], 2);
    assert_eq!(value["report"]["verified_proposal_files"], 3);
    assert_eq!(value["report"]["removed_orphans"], 1);
    assert_eq!(value["report"]["retained_unknown"], 4);
    assert_eq!(value["report"]["path_categories"]["version_graph"], 2);
    for forbidden in ["/private/data", "raw-secret-content", "SELECT "] {
        assert!(!event.contains(forbidden));
    }
}

#[tokio::test]
async fn startup_corrupt_reference_fails_before_orphan_cleanup() {
    let fixture = Fixture::new().await;
    let workspace = fixture
        .store
        .create_workspace("corrupt")
        .await
        .expect("workspace");
    let missing = format!("workspaces/{}/graphs/missing.json", workspace.id);
    let version = fixture
        .store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "corrupt",
            source: VersionSource::Manual,
            graph_path: &missing,
            graph_hash: &graph_hash(b"missing"),
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("version");
    let orphan = format!(
        "workspaces/{}/graphs/ops-{}.json",
        workspace.id,
        uuid::Uuid::now_v7().simple()
    );
    fixture.write(&orphan, b"preserve on failure").await;

    let error = reconcile_version_files(&fixture.store, fixture.root())
        .await
        .expect_err("missing referenced graph must fail closed");

    assert!(matches!(
        error,
        VersionFileReconciliationError::CorruptReference {
            ref record_id,
            path_category: "version_graph",
            ..
        } if record_id == &version.id
    ));
    assert!(fixture.root().join(orphan).exists());
    assert!(
        !error
            .to_string()
            .contains(fixture.root().to_string_lossy().as_ref())
    );
}

#[tokio::test]
async fn startup_delete_failure_is_visible_and_stops_cleanup() {
    let fixture = Fixture::new().await;
    let workspace = fixture
        .store
        .create_workspace("delete fault")
        .await
        .expect("workspace");
    let orphan = format!(
        "workspaces/{}/graphs/ops-{}.json",
        workspace.id,
        uuid::Uuid::now_v7().simple()
    );
    fixture.write(&orphan, b"preserve").await;
    let io = FaultIo;

    let error = reconcile_version_files_with_io(&fixture.store, fixture.root(), &io)
        .await
        .expect_err("delete failure must fail startup");

    assert!(matches!(
        error,
        VersionFileReconciliationError::FileSystem {
            operation: "remove_orphan",
            kind: std::io::ErrorKind::PermissionDenied,
        }
    ));
    assert!(fixture.root().join(orphan).exists());
}

#[tokio::test]
async fn startup_store_query_failure_preserves_owned_candidate() {
    let fixture = Fixture::new().await;
    let workspace = fixture
        .store
        .create_workspace("query fault")
        .await
        .expect("workspace");
    let orphan = format!(
        "workspaces/{}/graphs/ops-{}.json",
        workspace.id,
        uuid::Uuid::now_v7().simple()
    );
    fixture.write(&orphan, b"preserve").await;
    fixture.store.pool().close().await;

    let error = reconcile_version_files(&fixture.store, fixture.root())
        .await
        .expect_err("unknown reference state must fail startup");

    assert!(matches!(
        error,
        VersionFileReconciliationError::StoreQuery {
            operation: "enumerate_workspaces"
        }
    ));
    assert!(fixture.root().join(orphan).exists());
    assert!(!error.to_string().contains("SELECT"));
}

#[tokio::test]
async fn startup_rejects_proposal_traversal_before_reading_external_json() {
    let fixture = Fixture::new().await;
    let (workspace_id, version_id) = fixture.seed_valid_version().await;
    fixture
        .store
        .create_proposal(NewProposal {
            workspace_id: &workspace_id,
            base_version_id: &version_id,
            kind: "modify",
            title: "unsafe",
            summary: "unsafe",
            ops_path: "../external.json",
            preview_graph_path: None,
            message_id: None,
        })
        .await
        .expect("proposal");

    let error = reconcile_version_files(&fixture.store, fixture.root())
        .await
        .expect_err("traversal must fail startup");

    assert!(matches!(
        error,
        VersionFileReconciliationError::CorruptReference {
            code: "unsafe_path",
            path_category: "proposal_ops",
            ..
        }
    ));
}

#[tokio::test]
async fn startup_rejects_wrong_proposal_json_type() {
    let fixture = Fixture::new().await;
    let (workspace_id, version_id) = fixture.seed_valid_version().await;
    let ops_path = format!("workspaces/{workspace_id}/graphs/wrong-ops.json");
    fixture.write(&ops_path, br#"{"op":"not-an-array"}"#).await;
    fixture
        .store
        .create_proposal(NewProposal {
            workspace_id: &workspace_id,
            base_version_id: &version_id,
            kind: "modify",
            title: "wrong type",
            summary: "wrong type",
            ops_path: &ops_path,
            preview_graph_path: None,
            message_id: None,
        })
        .await
        .expect("proposal");

    let error = reconcile_version_files(&fixture.store, fixture.root())
        .await
        .expect_err("proposal ops must be an array of declared operations");

    assert!(matches!(
        error,
        VersionFileReconciliationError::CorruptReference {
            code: "invalid_json",
            path_category: "proposal_ops",
            ..
        }
    ));
}

#[tokio::test]
async fn startup_scan_failure_is_visible() {
    let fixture = Fixture::new().await;
    let file_root = fixture.root().join("not-a-directory");
    std::fs::write(&file_root, b"file").expect("file root");

    let error = reconcile_version_files(&fixture.store, &file_root)
        .await
        .expect_err("scan uncertainty must fail startup");

    assert!(matches!(
        error,
        VersionFileReconciliationError::FileSystem {
            operation: "scan_workspaces",
            ..
        }
    ));
}

#[tokio::test]
async fn startup_parent_sync_failure_is_visible_after_delete() {
    let fixture = Fixture::new().await;
    let workspace = fixture
        .store
        .create_workspace("sync fault")
        .await
        .expect("workspace");
    let orphan = format!(
        "workspaces/{}/graphs/ops-{}.json",
        workspace.id,
        uuid::Uuid::now_v7().simple()
    );
    fixture.write(&orphan, b"orphan").await;

    let error = reconcile_version_files_with_io(&fixture.store, fixture.root(), &SyncFaultIo)
        .await
        .expect_err("parent sync is part of durable cleanup");

    assert!(matches!(
        error,
        VersionFileReconciliationError::FileSystem {
            operation: "sync_orphan_parent",
            kind: std::io::ErrorKind::PermissionDenied,
        }
    ));
    assert!(!fixture.root().join(orphan).exists());
}

#[tokio::test]
async fn startup_reconciliation_precedes_stale_run_cleanup() {
    let fixture = Fixture::new().await;
    let workspace = fixture
        .store
        .create_workspace("ordering")
        .await
        .expect("workspace");
    let version = fixture
        .store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "missing",
            source: VersionSource::Manual,
            graph_path: "missing.json",
            graph_hash: &graph_hash(b"missing"),
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("version");
    let run = fixture
        .store
        .create_run(NewRun {
            workspace_id: &workspace.id,
            version_id: &version.id,
            group_id: None,
            label: "stale",
            trigger: "manual",
            plan_json: None,
            estimate_json: None,
            status: "running",
        })
        .await
        .expect("run");

    let error = match AppState::open_for_test(EventBus::new(16), fixture.root().to_path_buf()).await
    {
        Ok(_) => panic!("reconciliation must block later startup mutations"),
        Err(error) => error,
    };

    assert!(matches!(error, AppStateError::VersionFileConsistency(_)));
    assert_eq!(
        fixture
            .store
            .run(&run.id)
            .await
            .expect("unchanged run")
            .status,
        "running"
    );
}

#[tokio::test]
async fn legacy_retains_unknown_symlink_without_following_it() {
    let fixture = Fixture::new().await;
    let workspace = fixture
        .store
        .create_workspace("legacy")
        .await
        .expect("workspace");
    let external = tempfile::tempdir().expect("external dir");
    let target = external
        .path()
        .join(format!("ops-{}.json", uuid::Uuid::now_v7().simple()));
    std::fs::write(&target, b"external").expect("external file");
    let graph_dir = fixture
        .root()
        .join("workspaces")
        .join(&workspace.id)
        .join("graphs");
    std::fs::create_dir_all(&graph_dir).expect("graph dir");
    let link = graph_dir.join(format!("ops-{}.json", uuid::Uuid::now_v7().simple()));
    std::os::unix::fs::symlink(&target, &link).expect("symlink");

    let report = reconcile_version_files(&fixture.store, fixture.root())
        .await
        .expect("retain symlink");

    assert_eq!(report.removed_orphans, 0);
    assert_eq!(report.retained_unknown, 1);
    assert!(link.symlink_metadata().is_ok());
    assert_eq!(std::fs::read(target).expect("target"), b"external");
}

struct Fixture {
    store: Store,
    dir: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().expect("data dir");
        let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
        let store = Store::open(&database_url).await.expect("store");
        Self { store, dir }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    async fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.root().join(relative);
        tokio::fs::create_dir_all(path.parent().expect("parent"))
            .await
            .expect("parent");
        tokio::fs::write(path, bytes).await.expect("write");
    }

    async fn seed_valid_version(&self) -> (String, String) {
        let workspace = self
            .store
            .create_workspace("valid")
            .await
            .expect("workspace");
        let graph = sample_graph();
        let bytes = serde_json::to_vec(&graph).expect("graph JSON");
        let path = format!(
            "workspaces/{}/graphs/initial-{}.json",
            workspace.id,
            uuid::Uuid::now_v7().simple()
        );
        self.write(&path, &bytes).await;
        let version = self
            .store
            .create_version(NewVersion {
                workspace_id: &workspace.id,
                label: "initial",
                source: VersionSource::Manual,
                graph_path: &path,
                graph_hash: &graph_hash(&bytes),
                parent_id: None,
                semantics_json: None,
            })
            .await
            .expect("version");
        (workspace.id, version.id)
    }

    async fn seed_valid_proposal(&self, workspace_id: &str) {
        let versions = self
            .store
            .versions_for_workspace(workspace_id)
            .await
            .expect("versions");
        let ops_path = format!(
            "workspaces/{workspace_id}/graphs/proposal-ops-{}.json",
            uuid::Uuid::now_v7().simple()
        );
        let preview_path = format!(
            "workspaces/{workspace_id}/graphs/proposal-preview-{}.json",
            uuid::Uuid::now_v7().simple()
        );
        self.write(&ops_path, b"[]").await;
        self.write(
            &preview_path,
            &serde_json::to_vec(&sample_graph()).expect("preview JSON"),
        )
        .await;
        self.store
            .create_proposal(NewProposal {
                workspace_id,
                base_version_id: &versions[0].id,
                kind: "modify",
                title: "proposal",
                summary: "proposal",
                ops_path: &ops_path,
                preview_graph_path: Some(&preview_path),
                message_id: None,
            })
            .await
            .expect("proposal");
    }
}

fn sample_graph() -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::new(),
        edges: Vec::new(),
        catalog_revision: None,
    }
}

struct FaultIo;

impl ReconciliationIo for FaultIo {
    fn remove_file(&self, _path: &Path) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
    }

    fn sync_parent(&self, _parent: &Path) -> std::io::Result<()> {
        Ok(())
    }
}

struct SyncFaultIo;

impl ReconciliationIo for SyncFaultIo {
    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }

    fn sync_parent(&self, _parent: &Path) -> std::io::Result<()> {
        Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
    }
}
