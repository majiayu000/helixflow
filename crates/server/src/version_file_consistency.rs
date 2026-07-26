use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use helixflow_graph::WorkflowGraph;
use helixflow_store::{Store, VersionRecord};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::graph_files::{canonical_graph_bytes, graph_hash};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StrictSha256([u8; 32]);

impl StrictSha256 {
    pub(crate) fn parse(value: &str) -> Result<Self, VersionFileConsistencyError> {
        let Some(hex) = value.strip_prefix("sha256:") else {
            return Err(VersionFileConsistencyError::InvalidHash);
        };
        if hex.len() != 64
            || !hex
                .as_bytes()
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        {
            return Err(VersionFileConsistencyError::InvalidHash);
        }
        let mut digest = [0_u8; 32];
        for (index, chunk) in hex.as_bytes().chunks_exact(2).enumerate() {
            let high = hex_nibble(chunk[0]).ok_or(VersionFileConsistencyError::InvalidHash)?;
            let low = hex_nibble(chunk[1]).ok_or(VersionFileConsistencyError::InvalidHash)?;
            digest[index] = (high << 4) | low;
        }
        Ok(Self(digest))
    }

    fn matches(self, bytes: &[u8]) -> bool {
        let actual = Sha256::digest(bytes);
        actual.as_slice() == self.0
    }
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CandidateKind {
    Initial,
    Layout,
    Ops,
    ProposalApplied,
    ProposalOps,
    ProposalPreview,
}

impl CandidateKind {
    fn slug(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Layout => "layout",
            Self::Ops => "ops",
            Self::ProposalApplied => "proposal-applied",
            Self::ProposalOps => "proposal-ops",
            Self::ProposalPreview => "proposal-preview",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CandidateCleanupOutcome {
    Removed,
    PreservedReferenced,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VersionFileConsistencyError {
    InvalidHash,
    InvalidStoredHash {
        record_id: String,
        path_category: &'static str,
    },
    InvalidWorkspaceIdentity,
    EncodeJson,
    UnsafeRelativePath {
        record_id: String,
        path_category: &'static str,
    },
    MissingFile {
        record_id: String,
        path_category: &'static str,
    },
    HashMismatch {
        record_id: String,
        path_category: &'static str,
    },
    InvalidJson {
        record_id: String,
        path_category: &'static str,
    },
    Io {
        operation: &'static str,
        kind: io::ErrorKind,
    },
    CandidateState {
        operation: &'static str,
    },
    CleanupDeferred {
        operation: &'static str,
        kind: Option<io::ErrorKind>,
    },
    CleanupBatchDeferred {
        failure_count: usize,
        first_operation: &'static str,
        first_kind: Option<io::ErrorKind>,
    },
    PrimaryAndCleanup {
        primary_operation: &'static str,
        primary_kind: Option<io::ErrorKind>,
        cleanup_operation: &'static str,
        cleanup_kind: Option<io::ErrorKind>,
    },
}

impl std::fmt::Display for VersionFileConsistencyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHash => write!(f, "version graph hash is not canonical sha256"),
            Self::InvalidStoredHash {
                record_id,
                path_category,
            } => write!(f, "record `{record_id}` has invalid {path_category} sha256"),
            Self::InvalidWorkspaceIdentity => write!(f, "workspace identity is not path-safe"),
            Self::EncodeJson => write!(f, "candidate JSON encoding failed"),
            Self::UnsafeRelativePath {
                record_id,
                path_category,
            } => write!(
                f,
                "record `{record_id}` has unsafe {path_category} relative path"
            ),
            Self::MissingFile {
                record_id,
                path_category,
            } => write!(f, "record `{record_id}` is missing {path_category} payload"),
            Self::HashMismatch {
                record_id,
                path_category,
            } => write!(
                f,
                "record `{record_id}` has mismatched {path_category} hash"
            ),
            Self::InvalidJson {
                record_id,
                path_category,
            } => write!(f, "record `{record_id}` has invalid {path_category} JSON"),
            Self::Io { operation, kind } => {
                write!(
                    f,
                    "version file operation `{operation}` failed with {kind:?}"
                )
            }
            Self::CandidateState { operation } => {
                write!(f, "candidate state does not allow `{operation}`")
            }
            Self::CleanupDeferred { operation, kind } => write!(
                f,
                "candidate cleanup deferred during `{operation}` ({kind:?})"
            ),
            Self::CleanupBatchDeferred {
                failure_count,
                first_operation,
                first_kind,
            } => write!(
                f,
                "candidate cleanup deferred for {failure_count} candidate(s); first failure was `{first_operation}` ({first_kind:?})"
            ),
            Self::PrimaryAndCleanup {
                primary_operation,
                primary_kind,
                cleanup_operation,
                cleanup_kind,
            } => write!(
                f,
                "version file operation `{primary_operation}` failed ({primary_kind:?}); cleanup `{cleanup_operation}` also failed ({cleanup_kind:?})"
            ),
        }
    }
}

impl std::error::Error for VersionFileConsistencyError {}

pub(crate) async fn read_version_graph(
    data_dir: &Path,
    version: &VersionRecord,
) -> Result<WorkflowGraph, VersionFileConsistencyError> {
    read_verified_json(data_dir, version).await
}

async fn read_verified_json<T>(
    data_dir: &Path,
    version: &VersionRecord,
) -> Result<T, VersionFileConsistencyError>
where
    T: DeserializeOwned,
{
    let expected = StrictSha256::parse(&version.graph_hash).map_err(|_| {
        VersionFileConsistencyError::InvalidStoredHash {
            record_id: version.id.clone(),
            path_category: "version_graph",
        }
    })?;
    let relative = Path::new(&version.graph_path);
    if !is_safe_relative_path(relative) {
        return Err(VersionFileConsistencyError::UnsafeRelativePath {
            record_id: version.id.clone(),
            path_category: "version_graph",
        });
    }
    let root = tokio::fs::canonicalize(data_dir)
        .await
        .map_err(|error| io_error("canonicalize_data_root", error))?;
    let requested = root.join(relative);
    let canonical =
        tokio::fs::canonicalize(&requested)
            .await
            .map_err(|error| match error.kind() {
                io::ErrorKind::NotFound => VersionFileConsistencyError::MissingFile {
                    record_id: version.id.clone(),
                    path_category: "version_graph",
                },
                _ => io_error("canonicalize_version_graph", error),
            })?;
    if !canonical.starts_with(&root) {
        return Err(VersionFileConsistencyError::UnsafeRelativePath {
            record_id: version.id.clone(),
            path_category: "version_graph",
        });
    }
    let bytes = tokio::fs::read(&canonical)
        .await
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => VersionFileConsistencyError::MissingFile {
                record_id: version.id.clone(),
                path_category: "version_graph",
            },
            _ => io_error("read_version_graph", error),
        })?;
    if !expected.matches(&bytes) {
        return Err(VersionFileConsistencyError::HashMismatch {
            record_id: version.id.clone(),
            path_category: "version_graph",
        });
    }
    serde_json::from_slice(&bytes).map_err(|_| VersionFileConsistencyError::InvalidJson {
        record_id: version.id.clone(),
        path_category: "version_graph",
    })
}

fn io_error(operation: &'static str, error: io::Error) -> VersionFileConsistencyError {
    VersionFileConsistencyError::Io {
        operation,
        kind: error.kind(),
    }
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

pub(crate) trait CandidateIo {
    fn create_new(&self, path: &Path) -> io::Result<File>;
    fn write_all(&self, file: &mut File, bytes: &[u8]) -> io::Result<()>;
    fn sync_file(&self, file: &File) -> io::Result<()>;
    fn publish_no_replace(&self, temp: &Path, final_path: &Path) -> io::Result<()>;
    fn sync_parent(&self, parent: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FsCandidateIo;

impl CandidateIo for FsCandidateIo {
    fn create_new(&self, path: &Path) -> io::Result<File> {
        OpenOptions::new().write(true).create_new(true).open(path)
    }

    fn write_all(&self, file: &mut File, bytes: &[u8]) -> io::Result<()> {
        file.write_all(bytes)
    }

    fn sync_file(&self, file: &File) -> io::Result<()> {
        file.sync_all()
    }

    fn publish_no_replace(&self, temp: &Path, final_path: &Path) -> io::Result<()> {
        std::fs::hard_link(temp, final_path)
    }

    fn sync_parent(&self, parent: &Path) -> io::Result<()> {
        File::open(parent)?.sync_all()
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }
}

#[must_use = "candidate ownership requires explicit publish and store commit/cleanup coordination"]
#[derive(Debug)]
pub(crate) struct VersionFileCandidate {
    workspace_id: String,
    /// Consumed at construction time to derive file names; retained only for
    /// test assertions.
    #[cfg_attr(not(test), allow(dead_code))]
    kind: CandidateKind,
    relative_temp_path: PathBuf,
    relative_final_path: PathBuf,
    bytes: Vec<u8>,
    graph_hash: String,
    owned_temp: Option<PathBuf>,
    owned_final: Option<PathBuf>,
    published: bool,
    committed: bool,
}

impl VersionFileCandidate {
    pub(crate) fn from_graph(
        workspace_id: &str,
        kind: CandidateKind,
        graph: &WorkflowGraph,
    ) -> Result<Self, VersionFileConsistencyError> {
        let bytes =
            canonical_graph_bytes(graph).map_err(|_| VersionFileConsistencyError::EncodeJson)?;
        Self::from_bytes_with_ids(workspace_id, kind, bytes, Uuid::now_v7(), Uuid::now_v7())
    }

    pub(crate) fn from_keyed_ops_graph(
        workspace_id: &str,
        graph: &WorkflowGraph,
        opaque_digest: &str,
    ) -> Result<Self, VersionFileConsistencyError> {
        let digest_is_safe = opaque_digest.len() == 64
            && opaque_digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
        if !digest_is_safe {
            return Err(VersionFileConsistencyError::InvalidHash);
        }
        let bytes =
            canonical_graph_bytes(graph).map_err(|_| VersionFileConsistencyError::EncodeJson)?;
        Self::from_bytes_with_final_name(
            workspace_id,
            CandidateKind::Ops,
            bytes,
            format!("ops-key-{opaque_digest}.json"),
            Uuid::now_v7(),
        )
    }

    pub(crate) fn from_json<T>(
        workspace_id: &str,
        kind: CandidateKind,
        value: &T,
    ) -> Result<Self, VersionFileConsistencyError>
    where
        T: Serialize,
    {
        let bytes =
            serde_json::to_vec(value).map_err(|_| VersionFileConsistencyError::EncodeJson)?;
        Self::from_bytes_with_ids(workspace_id, kind, bytes, Uuid::now_v7(), Uuid::now_v7())
    }

    #[cfg(test)]
    pub(crate) fn from_graph_with_ids(
        workspace_id: &str,
        kind: CandidateKind,
        graph: &WorkflowGraph,
        final_id: Uuid,
        ownership_nonce: Uuid,
    ) -> Result<Self, VersionFileConsistencyError> {
        let bytes =
            canonical_graph_bytes(graph).map_err(|_| VersionFileConsistencyError::EncodeJson)?;
        Self::from_bytes_with_ids(workspace_id, kind, bytes, final_id, ownership_nonce)
    }

    fn from_bytes_with_ids(
        workspace_id: &str,
        kind: CandidateKind,
        bytes: Vec<u8>,
        final_id: Uuid,
        ownership_nonce: Uuid,
    ) -> Result<Self, VersionFileConsistencyError> {
        let final_name = format!("{}-{}.json", kind.slug(), final_id.simple());
        Self::from_bytes_with_final_name(workspace_id, kind, bytes, final_name, ownership_nonce)
    }

    fn from_bytes_with_final_name(
        workspace_id: &str,
        kind: CandidateKind,
        bytes: Vec<u8>,
        final_name: String,
        ownership_nonce: Uuid,
    ) -> Result<Self, VersionFileConsistencyError> {
        let workspace_is_safe = !workspace_id.is_empty()
            && workspace_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
        if !workspace_is_safe {
            return Err(VersionFileConsistencyError::InvalidWorkspaceIdentity);
        }
        let directory = PathBuf::from("workspaces")
            .join(workspace_id)
            .join("graphs");
        let temp_id = Uuid::now_v7().simple();
        let nonce = ownership_nonce.simple();
        let temp_name = format!(".hf-{}-{temp_id}-{nonce}.tmp", kind.slug());
        Ok(Self {
            workspace_id: workspace_id.to_owned(),
            kind,
            relative_temp_path: directory.join(temp_name),
            relative_final_path: directory.join(final_name),
            graph_hash: graph_hash(&bytes),
            bytes,
            owned_temp: None,
            owned_final: None,
            published: false,
            committed: false,
        })
    }

    #[cfg(test)]
    pub(crate) fn relative_path(&self) -> &Path {
        &self.relative_final_path
    }

    pub(crate) fn relative_path_text(&self) -> Result<&str, VersionFileConsistencyError> {
        self.relative_final_path
            .to_str()
            .ok_or(VersionFileConsistencyError::CandidateState {
                operation: "encode_candidate_relative_path",
            })
    }

    pub(crate) fn graph_hash(&self) -> &str {
        &self.graph_hash
    }

    #[cfg(test)]
    pub(crate) fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    #[cfg(test)]
    pub(crate) fn kind(&self) -> CandidateKind {
        self.kind
    }

    pub(crate) fn publish(&mut self, data_dir: &Path) -> Result<(), VersionFileConsistencyError> {
        self.publish_with_io(data_dir, &FsCandidateIo)
    }

    pub(crate) fn publish_with_io<I: CandidateIo>(
        &mut self,
        data_dir: &Path,
        io: &I,
    ) -> Result<(), VersionFileConsistencyError> {
        if self.published || self.owned_temp.is_some() || self.owned_final.is_some() {
            return Err(VersionFileConsistencyError::CandidateState {
                operation: "publish",
            });
        }
        std::fs::create_dir_all(data_dir).map_err(|error| io_error("create_data_root", error))?;
        let root = data_dir
            .canonicalize()
            .map_err(|error| io_error("canonicalize_data_root", error))?;
        let relative_parent = self.relative_final_path.parent().ok_or(
            VersionFileConsistencyError::CandidateState {
                operation: "resolve_candidate_parent",
            },
        )?;
        let mut parent = root.clone();
        for component in relative_parent.components() {
            let Component::Normal(component) = component else {
                return Err(VersionFileConsistencyError::UnsafeRelativePath {
                    record_id: self.workspace_id.clone(),
                    path_category: "candidate_graph",
                });
            };
            let requested = parent.join(component);
            match std::fs::create_dir(&requested) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(io_error("create_candidate_parent", error)),
            }
            let canonical = requested
                .canonicalize()
                .map_err(|error| io_error("canonicalize_candidate_parent", error))?;
            if !canonical.starts_with(&root) {
                return Err(VersionFileConsistencyError::UnsafeRelativePath {
                    record_id: self.workspace_id.clone(),
                    path_category: "candidate_graph",
                });
            }
            parent = canonical;
        }
        let temp_name = self.relative_temp_path.file_name().ok_or(
            VersionFileConsistencyError::CandidateState {
                operation: "resolve_candidate_temp_name",
            },
        )?;
        let final_name = self.relative_final_path.file_name().ok_or(
            VersionFileConsistencyError::CandidateState {
                operation: "resolve_candidate_final_name",
            },
        )?;
        let temp = parent.join(temp_name);
        let final_path = parent.join(final_name);
        let mut file = io
            .create_new(&temp)
            .map_err(|error| io_error("create_candidate_temp", error))?;
        self.owned_temp = Some(temp.clone());

        if let Err(error) = io.write_all(&mut file, &self.bytes) {
            return self.fail_and_cleanup(io_error("write_candidate_temp", error), io);
        }
        if let Err(error) = io.sync_file(&file) {
            return self.fail_and_cleanup(io_error("sync_candidate_temp", error), io);
        }
        drop(file);
        if let Err(error) = io.publish_no_replace(&temp, &final_path) {
            return self.fail_and_cleanup(io_error("publish_candidate", error), io);
        }
        self.owned_final = Some(final_path);
        if let Err(error) = io.sync_parent(&parent) {
            return self.fail_and_cleanup(io_error("sync_candidate_parent", error), io);
        }
        if let Err(error) = io.remove_file(&temp) {
            return self.fail_and_cleanup(io_error("remove_candidate_temp", error), io);
        }
        self.owned_temp = None;
        self.published = true;
        Ok(())
    }

    pub(crate) fn mark_committed(&mut self) -> Result<(), VersionFileConsistencyError> {
        if !self.published {
            return Err(VersionFileConsistencyError::CandidateState {
                operation: "mark_committed",
            });
        }
        self.committed = true;
        Ok(())
    }

    pub(crate) async fn cleanup_after_store_error(
        &mut self,
        store: &Store,
    ) -> Result<CandidateCleanupOutcome, VersionFileConsistencyError> {
        self.cleanup_after_store_error_with_io(store, &FsCandidateIo)
            .await
    }

    pub(crate) async fn cleanup_after_store_error_with_io<I: CandidateIo>(
        &mut self,
        store: &Store,
        io: &I,
    ) -> Result<CandidateCleanupOutcome, VersionFileConsistencyError> {
        if self.committed {
            return Err(VersionFileConsistencyError::CandidateState {
                operation: "cleanup_committed_candidate",
            });
        }
        let path = self.relative_path_text()?.to_owned();
        let versions = store.version_file_references(&path).await.map_err(|_| {
            VersionFileConsistencyError::CleanupDeferred {
                operation: "query_version_references",
                kind: None,
            }
        })?;
        let proposals = store.proposal_file_references(&path).await.map_err(|_| {
            VersionFileConsistencyError::CleanupDeferred {
                operation: "query_proposal_references",
                kind: None,
            }
        })?;
        if !versions.is_empty() || !proposals.is_empty() {
            return Ok(CandidateCleanupOutcome::PreservedReferenced);
        }
        self.cleanup_owned_with_io(io)?;
        Ok(CandidateCleanupOutcome::Removed)
    }

    fn fail_and_cleanup<I: CandidateIo>(
        &mut self,
        primary: VersionFileConsistencyError,
        io: &I,
    ) -> Result<(), VersionFileConsistencyError> {
        match self.cleanup_owned_with_io(io) {
            Ok(()) => Err(primary),
            Err(cleanup) => Err(combine_primary_and_cleanup(primary, cleanup)),
        }
    }

    fn cleanup_owned_with_io<I: CandidateIo>(
        &mut self,
        io: &I,
    ) -> Result<(), VersionFileConsistencyError> {
        let mut first_error = None;
        let parent = self
            .owned_final
            .as_deref()
            .or(self.owned_temp.as_deref())
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        let mut removed_any = false;
        if let Some(path) = self.owned_final.as_ref() {
            match io.remove_file(path) {
                Ok(()) => {
                    self.owned_final = None;
                    removed_any = true;
                }
                Err(error) => first_error = Some(error.kind()),
            }
        }
        if let Some(path) = self.owned_temp.as_ref() {
            match io.remove_file(path) {
                Ok(()) => {
                    self.owned_temp = None;
                    removed_any = true;
                }
                Err(error) if first_error.is_none() => first_error = Some(error.kind()),
                Err(_) => {}
            }
        }
        if removed_any
            && let Some(parent) = parent
            && let Err(error) = io.sync_parent(&parent)
            && first_error.is_none()
        {
            first_error = Some(error.kind());
        }
        if let Some(kind) = first_error {
            return Err(VersionFileConsistencyError::CleanupDeferred {
                operation: "remove_owned_candidate",
                kind: Some(kind),
            });
        }
        self.published = false;
        Ok(())
    }
}

fn combine_primary_and_cleanup(
    primary: VersionFileConsistencyError,
    cleanup: VersionFileConsistencyError,
) -> VersionFileConsistencyError {
    let (primary_operation, primary_kind) = error_descriptor(&primary);
    let (cleanup_operation, cleanup_kind) = error_descriptor(&cleanup);
    VersionFileConsistencyError::PrimaryAndCleanup {
        primary_operation,
        primary_kind,
        cleanup_operation,
        cleanup_kind,
    }
}

fn error_descriptor(error: &VersionFileConsistencyError) -> (&'static str, Option<io::ErrorKind>) {
    match error {
        VersionFileConsistencyError::Io { operation, kind } => (operation, Some(*kind)),
        VersionFileConsistencyError::CleanupDeferred { operation, kind } => (*operation, *kind),
        VersionFileConsistencyError::CleanupBatchDeferred {
            first_operation,
            first_kind,
            ..
        } => (*first_operation, *first_kind),
        VersionFileConsistencyError::CandidateState { operation } => (operation, None),
        VersionFileConsistencyError::PrimaryAndCleanup {
            primary_operation,
            primary_kind,
            ..
        } => (primary_operation, *primary_kind),
        _ => ("version_file_consistency", None),
    }
}

impl Drop for VersionFileCandidate {
    fn drop(&mut self) {
        // This is only a best-effort safety net for an abandoned temporary file. Coordinators
        // must call cleanup_after_store_error so durable references are checked and failures are
        // returned; Drop cannot safely remove published files or report cleanup errors.
        if let Some(temp) = self.owned_temp.take() {
            let _ = std::fs::remove_file(temp);
        }
    }
}

#[must_use = "candidate sets require explicit store commit/cleanup coordination"]
#[derive(Debug, Default)]
pub(crate) struct VersionFileCandidateSet {
    candidates: Vec<VersionFileCandidate>,
}

impl VersionFileCandidateSet {
    pub(crate) fn push(&mut self, candidate: VersionFileCandidate) {
        self.candidates.push(candidate);
    }

    #[cfg(test)]
    pub(crate) fn candidates(&self) -> &[VersionFileCandidate] {
        &self.candidates
    }

    pub(crate) fn publish_all(
        &mut self,
        data_dir: &Path,
    ) -> Result<(), VersionFileConsistencyError> {
        self.publish_all_with_io(data_dir, &FsCandidateIo)
    }

    pub(crate) fn publish_all_with_io<I: CandidateIo>(
        &mut self,
        data_dir: &Path,
        io: &I,
    ) -> Result<(), VersionFileConsistencyError> {
        for index in 0..self.candidates.len() {
            if let Err(error) = self.candidates[index].publish_with_io(data_dir, io) {
                let mut cleanup_error = None;
                for candidate in self.candidates[..index].iter_mut().rev() {
                    if let Err(error) = candidate.cleanup_owned_with_io(io) {
                        cleanup_error.get_or_insert(error);
                    }
                }
                return Err(match cleanup_error {
                    Some(cleanup) => combine_primary_and_cleanup(error, cleanup),
                    None => error,
                });
            }
        }
        Ok(())
    }

    pub(crate) fn mark_all_committed(&mut self) -> Result<(), VersionFileConsistencyError> {
        for candidate in &mut self.candidates {
            candidate.mark_committed()?;
        }
        Ok(())
    }

    pub(crate) async fn cleanup_after_store_error(
        &mut self,
        store: &Store,
    ) -> Result<Vec<CandidateCleanupOutcome>, VersionFileConsistencyError> {
        self.cleanup_after_store_error_with_io(store, &FsCandidateIo)
            .await
    }

    pub(crate) async fn cleanup_after_store_error_with_io<I: CandidateIo>(
        &mut self,
        store: &Store,
        io: &I,
    ) -> Result<Vec<CandidateCleanupOutcome>, VersionFileConsistencyError> {
        let mut outcomes = Vec::with_capacity(self.candidates.len());
        let mut failure_count = 0;
        let mut first_failure = None;
        for candidate in &mut self.candidates {
            match candidate.cleanup_after_store_error_with_io(store, io).await {
                Ok(outcome) => outcomes.push(outcome),
                Err(error) => {
                    failure_count += 1;
                    first_failure.get_or_insert_with(|| error_descriptor(&error));
                }
            }
        }
        if let Some((first_operation, first_kind)) = first_failure {
            return Err(VersionFileConsistencyError::CleanupBatchDeferred {
                failure_count,
                first_operation,
                first_kind,
            });
        }
        Ok(outcomes)
    }
}
