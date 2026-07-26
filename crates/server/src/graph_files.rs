use std::collections::BTreeMap;
use std::path::{Component, Path};

use helixflow_graph::WorkflowGraph;
#[cfg(test)]
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

use crate::api_error::ApiError;

pub(crate) fn blank_graph() -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::new(),
        edges: Vec::new(),
        catalog_revision: None,
    }
}

pub(crate) async fn read_graph_file(
    data_dir: &Path,
    graph_path: &str,
) -> Result<WorkflowGraph, ApiError> {
    read_json_file(data_dir, graph_path, "read stored graph").await
}

pub(crate) async fn read_json_file<T>(
    data_dir: &Path,
    relative_path: &str,
    context: &str,
) -> Result<T, ApiError>
where
    T: DeserializeOwned,
{
    let full_path = safe_read_path(data_dir, relative_path)?;
    let bytes = tokio::fs::read(&full_path)
        .await
        .map_err(|err| ApiError::io(format!("{context} `{relative_path}`"), err))?;
    serde_json::from_slice(&bytes).map_err(|err| {
        ApiError::server_error(format!(
            "{context} `{relative_path}` contains invalid JSON: {err}"
        ))
    })
}

/// Test fixture writer. Production graph writes go through the
/// `VersionFileCandidate` publish/commit flow instead.
#[cfg(test)]
pub(crate) async fn write_json_file<T>(
    data_dir: &Path,
    relative_path: &Path,
    value: &T,
    context: &str,
) -> Result<String, ApiError>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|err| ApiError::server_error(format!("{context}: encode JSON: {err}")))?;
    let full_path = safe_write_path(data_dir, relative_path, context).await?;
    tokio::fs::write(&full_path, &bytes)
        .await
        .map_err(|err| ApiError::io(format!("{context}: write file"), err))?;
    Ok(graph_hash(&bytes))
}

pub(crate) fn graph_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hash = String::with_capacity("sha256:".len() + digest.len() * 2);
    hash.push_str("sha256:");
    for byte in digest {
        hash.push_str(&format!("{byte:02x}"));
    }
    hash
}

pub(crate) fn canonical_graph_bytes(graph: &WorkflowGraph) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(graph)
}

fn safe_read_path(data_dir: &Path, relative_path: &str) -> Result<std::path::PathBuf, ApiError> {
    let relative = Path::new(relative_path);
    if !is_safe_relative_path(relative) {
        return Err(ApiError::bad_request(format!(
            "stored path is not a safe relative path: {relative_path}"
        )));
    }

    let data_root = data_dir
        .canonicalize()
        .map_err(|err| ApiError::io("canonicalize HELIXFLOW_DATA_DIR", err))?;
    let full_path = data_root.join(relative);
    let canonical = full_path
        .canonicalize()
        .map_err(|err| ApiError::io(format!("resolve stored path `{relative_path}`"), err))?;
    if !canonical.starts_with(&data_root) {
        return Err(ApiError::bad_request(format!(
            "stored path escapes HELIXFLOW_DATA_DIR: {relative_path}"
        )));
    }
    Ok(canonical)
}

#[cfg(test)]
async fn safe_write_path(
    data_dir: &Path,
    relative_path: &Path,
    context: &str,
) -> Result<std::path::PathBuf, ApiError> {
    if !is_safe_relative_path(relative_path) {
        return Err(ApiError::bad_request(format!(
            "{context}: output path is not a safe relative path: {}",
            relative_path.display()
        )));
    }
    tokio::fs::create_dir_all(data_dir)
        .await
        .map_err(|err| ApiError::io("create HELIXFLOW_DATA_DIR", err))?;
    let data_root = data_dir
        .canonicalize()
        .map_err(|err| ApiError::io("canonicalize HELIXFLOW_DATA_DIR", err))?;
    let full_path = data_root.join(relative_path);
    if let Some(parent) = full_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| ApiError::io(format!("{context}: create parent directory"), err))?;
    }
    Ok(full_path)
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}
