use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use helixflow_graph::{ProposalOp, WorkflowGraph};
use helixflow_store::{ProposalRecord, Store, VersionRecord};
use serde::Serialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::graph_files::is_safe_relative_path;
use crate::version_file_consistency::{VersionFileConsistencyError, read_version_graph};

const CANDIDATE_KINDS: [&str; 6] = [
    "initial",
    "migration",
    "ops",
    "proposal-applied",
    "proposal-ops",
    "proposal-preview",
];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(crate) struct ReconciliationReport {
    pub(crate) verified_versions: usize,
    pub(crate) verified_proposal_files: usize,
    pub(crate) removed_orphans: usize,
    pub(crate) retained_unknown: usize,
    pub(crate) corrupt_references: usize,
    pub(crate) path_categories: BTreeMap<String, usize>,
}

impl ReconciliationReport {
    fn count(&mut self, category: &str) {
        *self.path_categories.entry(category.to_owned()).or_default() += 1;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VersionFileReconciliationError {
    StoreQuery {
        operation: &'static str,
    },
    CorruptReference {
        code: &'static str,
        record_id: String,
        path_category: &'static str,
    },
    FileSystem {
        operation: &'static str,
        kind: io::ErrorKind,
    },
}

impl std::fmt::Display for VersionFileReconciliationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StoreQuery { operation } => {
                write!(
                    f,
                    "version file reconciliation store query `{operation}` failed"
                )
            }
            Self::CorruptReference {
                code,
                record_id,
                path_category,
            } => write!(
                f,
                "version file reconciliation rejected record `{record_id}` ({path_category}, {code})"
            ),
            Self::FileSystem { operation, kind } => write!(
                f,
                "version file reconciliation filesystem operation `{operation}` failed with {kind:?}"
            ),
        }
    }
}

impl std::error::Error for VersionFileReconciliationError {}

pub(crate) trait ReconciliationIo {
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn sync_parent(&self, parent: &Path) -> io::Result<()>;
}

#[derive(Debug, Clone, Copy)]
struct FsReconciliationIo;

impl ReconciliationIo for FsReconciliationIo {
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn sync_parent(&self, parent: &Path) -> io::Result<()> {
        File::open(parent)?.sync_all()
    }
}

pub(crate) async fn reconcile_version_files(
    store: &Store,
    data_dir: &Path,
) -> Result<ReconciliationReport, VersionFileReconciliationError> {
    reconcile_version_files_with_io(store, data_dir, &FsReconciliationIo).await
}

pub(crate) async fn reconcile_version_files_with_io<I: ReconciliationIo>(
    store: &Store,
    data_dir: &Path,
    io: &I,
) -> Result<ReconciliationReport, VersionFileReconciliationError> {
    let mut report = ReconciliationReport::default();
    let referenced_targets = verify_references(store, data_dir, &mut report).await?;
    scan_and_remove_orphans(store, data_dir, io, &referenced_targets, &mut report).await?;
    Ok(report)
}

async fn verify_references(
    store: &Store,
    data_dir: &Path,
    report: &mut ReconciliationReport,
) -> Result<HashSet<PathBuf>, VersionFileReconciliationError> {
    let mut referenced_targets = HashSet::new();
    let workspaces = store
        .workspaces()
        .await
        .map_err(|_| store_query("enumerate_workspaces"))?;
    for workspace in workspaces {
        let versions = store
            .versions_for_workspace(&workspace.id)
            .await
            .map_err(|_| store_query("enumerate_versions"))?;
        for version in versions {
            read_version_graph(data_dir, &version)
                .await
                .map_err(|error| map_version_error(&version, error))?;
            referenced_targets.insert(
                canonical_reference_target(
                    data_dir,
                    &version.graph_path,
                    &version.id,
                    "version_graph",
                )
                .await?,
            );
            report.verified_versions += 1;
            report.count("version_graph");
        }
        let proposals = store
            .workspace_proposals(&workspace.id)
            .await
            .map_err(|_| store_query("enumerate_proposals"))?;
        for proposal in proposals {
            referenced_targets.extend(verify_proposal_files(data_dir, &proposal).await?);
            report.verified_proposal_files += 1;
            report.count("proposal_ops");
            if proposal.preview_graph_path.is_some() {
                report.verified_proposal_files += 1;
                report.count("proposal_preview");
            }
        }
    }
    Ok(referenced_targets)
}

async fn verify_proposal_files(
    data_dir: &Path,
    proposal: &ProposalRecord,
) -> Result<Vec<PathBuf>, VersionFileReconciliationError> {
    let mut targets = vec![
        read_safe_json::<Vec<ProposalOp>>(
            data_dir,
            &proposal.ops_path,
            &proposal.id,
            "proposal_ops",
        )
        .await?,
    ];
    if let Some(path) = &proposal.preview_graph_path {
        targets.push(
            read_safe_json::<WorkflowGraph>(data_dir, path, &proposal.id, "proposal_preview")
                .await?,
        );
    }
    Ok(targets)
}

async fn read_safe_json<T: DeserializeOwned>(
    data_dir: &Path,
    relative_path: &str,
    record_id: &str,
    path_category: &'static str,
) -> Result<PathBuf, VersionFileReconciliationError> {
    let canonical =
        canonical_reference_target(data_dir, relative_path, record_id, path_category).await?;
    let bytes = tokio::fs::read(&canonical)
        .await
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => corrupt("missing_file", record_id, path_category),
            _ => fs_error("read_proposal_file", error),
        })?;
    serde_json::from_slice::<T>(&bytes)
        .map_err(|_| corrupt("invalid_json", record_id, path_category))?;
    Ok(canonical)
}

async fn canonical_reference_target(
    data_dir: &Path,
    relative_path: &str,
    record_id: &str,
    path_category: &'static str,
) -> Result<PathBuf, VersionFileReconciliationError> {
    let relative = Path::new(relative_path);
    if !is_safe_relative_path(relative) {
        return Err(corrupt("unsafe_path", record_id, path_category));
    }
    let root = tokio::fs::canonicalize(data_dir)
        .await
        .map_err(|error| fs_error("canonicalize_data_root", error))?;
    let canonical =
        tokio::fs::canonicalize(root.join(relative))
            .await
            .map_err(|error| match error.kind() {
                io::ErrorKind::NotFound => corrupt("missing_file", record_id, path_category),
                _ => fs_error("canonicalize_proposal_file", error),
            })?;
    if !canonical.starts_with(&root) {
        return Err(corrupt("unsafe_path", record_id, path_category));
    }
    Ok(canonical)
}

async fn scan_and_remove_orphans<I: ReconciliationIo>(
    store: &Store,
    data_dir: &Path,
    io: &I,
    referenced_targets: &HashSet<PathBuf>,
    report: &mut ReconciliationReport,
) -> Result<(), VersionFileReconciliationError> {
    let root = std::fs::canonicalize(data_dir)
        .map_err(|error| fs_error("canonicalize_scan_root", error))?;
    let workspaces = root.join("workspaces");
    let workspace_entries = match std::fs::read_dir(&workspaces) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(fs_error("scan_workspaces", error)),
    };
    for workspace_entry in workspace_entries {
        let workspace_entry =
            workspace_entry.map_err(|error| fs_error("scan_workspace_entry", error))?;
        let workspace_type = workspace_entry
            .file_type()
            .map_err(|error| fs_error("classify_workspace_entry", error))?;
        if !workspace_type.is_dir() || workspace_type.is_symlink() {
            retain_unknown(report, "workspace_entry");
            continue;
        }
        scan_graph_directory(
            store,
            &root,
            &workspace_entry.path(),
            io,
            referenced_targets,
            report,
        )
        .await?;
    }
    Ok(())
}

async fn scan_graph_directory<I: ReconciliationIo>(
    store: &Store,
    root: &Path,
    workspace_dir: &Path,
    io: &I,
    referenced_targets: &HashSet<PathBuf>,
    report: &mut ReconciliationReport,
) -> Result<(), VersionFileReconciliationError> {
    let graphs = workspace_dir.join("graphs");
    let graph_metadata = match std::fs::symlink_metadata(&graphs) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(fs_error("inspect_graph_directory", error)),
    };
    if !graph_metadata.is_dir() || graph_metadata.file_type().is_symlink() {
        retain_unknown(report, "graph_directory");
        return Ok(());
    }
    let canonical_graphs = std::fs::canonicalize(&graphs)
        .map_err(|error| fs_error("canonicalize_graph_directory", error))?;
    if !canonical_graphs.starts_with(root) {
        retain_unknown(report, "graph_directory");
        return Ok(());
    }
    let entries = std::fs::read_dir(&canonical_graphs)
        .map_err(|error| fs_error("scan_graph_directory", error))?;
    for entry in entries {
        let entry = entry.map_err(|error| fs_error("scan_graph_entry", error))?;
        let file_type = entry
            .file_type()
            .map_err(|error| fs_error("classify_graph_entry", error))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            retain_unknown(report, "unknown_graph_file");
            continue;
        };
        if !file_type.is_file() || file_type.is_symlink() || !is_owned_candidate_name(&name) {
            retain_unknown(report, "unknown_graph_file");
            continue;
        }
        let path = entry.path();
        let relative = path.strip_prefix(root).map_err(|_| {
            fs_kind(
                "resolve_candidate_relative_path",
                io::ErrorKind::InvalidData,
            )
        })?;
        let relative_text = relative
            .to_str()
            .ok_or_else(|| fs_kind("encode_candidate_relative_path", io::ErrorKind::InvalidData))?
            .replace(std::path::MAIN_SEPARATOR, "/");
        let versions = store
            .version_file_references(&relative_text)
            .await
            .map_err(|_| store_query("query_version_references"))?;
        let proposals = store
            .proposal_file_references(&relative_text)
            .await
            .map_err(|_| store_query("query_proposal_references"))?;
        if !versions.is_empty() || !proposals.is_empty() {
            report.count("referenced_candidate");
            continue;
        }
        let canonical_candidate = std::fs::canonicalize(&path)
            .map_err(|error| fs_error("canonicalize_candidate", error))?;
        if referenced_targets.contains(&canonical_candidate) {
            report.count("aliased_reference_candidate");
            continue;
        }
        io.remove_file(&path)
            .map_err(|error| fs_error("remove_orphan", error))?;
        io.sync_parent(&canonical_graphs)
            .map_err(|error| fs_error("sync_orphan_parent", error))?;
        report.removed_orphans += 1;
        report.count("owned_orphan");
    }
    Ok(())
}

fn is_owned_candidate_name(name: &str) -> bool {
    CANDIDATE_KINDS
        .iter()
        .any(|kind| is_final_candidate(name, kind) || is_temporary_candidate(name, kind))
}

fn is_final_candidate(name: &str, kind: &str) -> bool {
    name.strip_prefix(&format!("{kind}-"))
        .and_then(|value| value.strip_suffix(".json"))
        .is_some_and(is_simple_uuid)
}

fn is_temporary_candidate(name: &str, kind: &str) -> bool {
    let Some(value) = name
        .strip_prefix(&format!(".hf-{kind}-"))
        .and_then(|value| value.strip_suffix(".tmp"))
    else {
        return false;
    };
    let Some((temp_id, nonce)) = value.split_once('-') else {
        return false;
    };
    is_simple_uuid(temp_id) && is_simple_uuid(nonce)
}

fn is_simple_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 32
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        && bytes[12] == b'7'
        && matches!(bytes[16], b'8' | b'9' | b'a' | b'b')
        && Uuid::parse_str(value).is_ok()
}

fn map_version_error(
    version: &VersionRecord,
    error: VersionFileConsistencyError,
) -> VersionFileReconciliationError {
    let (code, record_id, path_category) = match error {
        VersionFileConsistencyError::InvalidStoredHash {
            record_id,
            path_category,
        } => ("invalid_hash", record_id, path_category),
        VersionFileConsistencyError::UnsafeRelativePath {
            record_id,
            path_category,
        } => ("unsafe_path", record_id, path_category),
        VersionFileConsistencyError::MissingFile {
            record_id,
            path_category,
        } => ("missing_file", record_id, path_category),
        VersionFileConsistencyError::HashMismatch {
            record_id,
            path_category,
        } => ("hash_mismatch", record_id, path_category),
        VersionFileConsistencyError::InvalidJson {
            record_id,
            path_category,
        } => ("invalid_json", record_id, path_category),
        VersionFileConsistencyError::Io { .. } => {
            ("io_failure", version.id.clone(), "version_graph")
        }
        _ => ("invalid_reference", version.id.clone(), "version_graph"),
    };
    corrupt(code, &record_id, path_category)
}

fn retain_unknown(report: &mut ReconciliationReport, category: &str) {
    report.retained_unknown += 1;
    report.count(category);
}

fn corrupt(
    code: &'static str,
    record_id: &str,
    path_category: &'static str,
) -> VersionFileReconciliationError {
    VersionFileReconciliationError::CorruptReference {
        code,
        record_id: record_id.to_owned(),
        path_category,
    }
}

fn store_query(operation: &'static str) -> VersionFileReconciliationError {
    VersionFileReconciliationError::StoreQuery { operation }
}

fn fs_error(operation: &'static str, error: io::Error) -> VersionFileReconciliationError {
    fs_kind(operation, error.kind())
}

fn fs_kind(operation: &'static str, kind: io::ErrorKind) -> VersionFileReconciliationError {
    VersionFileReconciliationError::FileSystem { operation, kind }
}
