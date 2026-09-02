//! Durable graph-file + SQLite commit coordination.

use std::future::Future;
use std::path::Path;

use helixflow_store::Store;

use crate::version_file_consistency::{
    VersionFileCandidate, VersionFileCandidateSet, VersionFileConsistencyError,
};

#[derive(Debug)]
pub(crate) enum VersionCommitError<E> {
    Consistency(VersionFileConsistencyError),
    Store {
        source: E,
        cleanup: Option<VersionFileConsistencyError>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VersionCommitDisposition {
    Committed,
    Replayed,
}

pub(crate) async fn commit_version_candidate<T, E>(
    candidate: &mut VersionFileCandidate,
    data_dir: &Path,
    store: &Store,
    transaction: impl Future<Output = Result<T, E>>,
) -> Result<T, VersionCommitError<E>> {
    commit_version_candidate_with_disposition(candidate, data_dir, store, async {
        transaction
            .await
            .map(|value| (value, VersionCommitDisposition::Committed))
    })
    .await
}

pub(crate) async fn commit_version_candidate_with_disposition<T, E>(
    candidate: &mut VersionFileCandidate,
    data_dir: &Path,
    store: &Store,
    transaction: impl Future<Output = Result<(T, VersionCommitDisposition), E>>,
) -> Result<T, VersionCommitError<E>> {
    candidate
        .publish(data_dir)
        .map_err(VersionCommitError::Consistency)?;
    match transaction.await {
        Ok((value, disposition)) => {
            match disposition {
                VersionCommitDisposition::Committed => candidate
                    .mark_committed()
                    .map_err(VersionCommitError::Consistency)?,
                VersionCommitDisposition::Replayed => {
                    candidate
                        .cleanup_after_store_error(store)
                        .await
                        .map_err(VersionCommitError::Consistency)?;
                }
            }
            Ok(value)
        }
        Err(source) => {
            let cleanup = candidate.cleanup_after_store_error(store).await.err();
            Err(VersionCommitError::Store { source, cleanup })
        }
    }
}

pub(crate) async fn commit_version_candidates<T, E>(
    candidates: &mut VersionFileCandidateSet,
    data_dir: &Path,
    store: &Store,
    transaction: impl Future<Output = Result<T, E>>,
) -> Result<T, VersionCommitError<E>> {
    candidates
        .publish_all(data_dir)
        .map_err(VersionCommitError::Consistency)?;
    match transaction.await {
        Ok(value) => {
            candidates
                .mark_all_committed()
                .map_err(VersionCommitError::Consistency)?;
            Ok(value)
        }
        Err(source) => {
            let cleanup = candidates.cleanup_after_store_error(store).await.err();
            Err(VersionCommitError::Store { source, cleanup })
        }
    }
}

/// Commit a new version that references an already durable, verified graph
/// snapshot (restore/undo) without manufacturing a duplicate graph file.
pub(crate) async fn commit_existing_version<T, E>(
    transaction: impl Future<Output = Result<T, E>>,
) -> Result<T, VersionCommitError<E>> {
    transaction
        .await
        .map_err(|source| VersionCommitError::Store {
            source,
            cleanup: None,
        })
}
