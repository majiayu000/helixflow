//! v1→v2 graph migration entry points (GH144).
//!
//! Dry-run is a pure, deterministic projection of `migrate_v1_to_v2`; apply
//! atomically creates a new version carrying the semantic layer. The
//! original version stays in history, so rollback is a plain restore.

use axum::Json;
use axum::extract::{Path, State};
use helixflow_graph::WorkflowGraph;
use helixflow_graph::graph_v2::{MigrationAction, MigrationReport, migrate_v1_to_v2};
use helixflow_registry::NodeRegistry;
use helixflow_store::{NewVersion, VersionRecord, VersionSource};
use serde::Serialize;
use serde_json::json;

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::version_file_consistency::{
    CandidateKind, VersionFileCandidate, VersionFileCandidateSet, read_version_graph,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MigrationDryRunResponse {
    pub(crate) status: MigrationStatus,
    pub(crate) version_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) report: Option<MigrationReport>,
    pub(crate) counts: MigrationCounts,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MigrationApplyResponse {
    pub(crate) status: MigrationStatus,
    pub(crate) version_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) migrated_version_id: Option<String>,
    pub(crate) counts: MigrationCounts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MigrationStatus {
    Ready,
    NeedsResolution,
    AlreadyMigrated,
    Migrated,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MigrationCounts {
    pub(crate) mapped: usize,
    pub(crate) structural: usize,
    pub(crate) needs_resolution: usize,
}

pub(crate) async fn migration_dry_run(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<MigrationDryRunResponse>, ApiError> {
    let (current, graph) = current_version_graph(&state, &workspace_id).await?;
    if current.semantics_json.is_some() {
        return Ok(Json(MigrationDryRunResponse {
            status: MigrationStatus::AlreadyMigrated,
            version_id: current.id,
            report: None,
            counts: MigrationCounts::default(),
        }));
    }

    let registry = NodeRegistry::builtin();
    let (_migrated, report) = migrate_v1_to_v2(&graph, &registry, helixflow_run::shared_catalog());
    let counts = count_actions(&report);
    Ok(Json(MigrationDryRunResponse {
        status: if report.resolvable {
            MigrationStatus::Ready
        } else {
            MigrationStatus::NeedsResolution
        },
        version_id: current.id,
        report: Some(report),
        counts,
    }))
}

pub(crate) async fn migration_apply(
    Path(workspace_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<MigrationApplyResponse>, ApiError> {
    let (current, graph) = current_version_graph(&state, &workspace_id).await?;
    if current.semantics_json.is_some() {
        return Ok(Json(MigrationApplyResponse {
            status: MigrationStatus::AlreadyMigrated,
            version_id: current.id,
            migrated_version_id: None,
            counts: MigrationCounts::default(),
        }));
    }

    let registry = NodeRegistry::builtin();
    let (migrated, report) = migrate_v1_to_v2(&graph, &registry, helixflow_run::shared_catalog());
    let counts = count_actions(&report);
    let Some(migrated) = migrated else {
        return Err(ApiError::conflict_with_details(
            "graph has nodes that need manual resolution before migration".to_owned(),
            json!({ "code": "MIGRATION_NEEDS_RESOLUTION", "report": report }),
        ));
    };

    let candidate = VersionFileCandidate::from_graph(
        &workspace_id,
        CandidateKind::MigratedGraph,
        &migrated.base,
    )
    .map_err(|err| ApiError::server_error(err.to_string()))?;
    let graph_path = candidate
        .relative_path_text()
        .map_err(|err| ApiError::server_error(err.to_string()))?
        .to_owned();
    let graph_hash = candidate.graph_hash().to_owned();
    let mut candidates = VersionFileCandidateSet::default();
    candidates.push(candidate);
    candidates
        .publish_all(&state.data_dir)
        .map_err(|err| ApiError::server_error(err.to_string()))?;

    let semantics_json = serde_json::to_string(&migrated.semantics)
        .map_err(|err| ApiError::server_error(err.to_string()))?;
    let version = state
        .store
        .create_version_after(
            NewVersion {
                workspace_id: &workspace_id,
                label: "v1→v2 migration",
                source: VersionSource::Manual,
                graph_path: &graph_path,
                graph_hash: &graph_hash,
                parent_id: Some(&current.id),
                semantics_json: Some(&semantics_json),
            },
            &current.id,
        )
        .await
        .map_err(ApiError::store)?;

    Ok(Json(MigrationApplyResponse {
        status: MigrationStatus::Migrated,
        version_id: current.id,
        migrated_version_id: Some(version.id),
        counts,
    }))
}

async fn current_version_graph(
    state: &AppState,
    workspace_id: &str,
) -> Result<(VersionRecord, WorkflowGraph), ApiError> {
    let workspace = state
        .store
        .workspace(workspace_id)
        .await
        .map_err(ApiError::store)?;
    let version_id = workspace
        .cur_version_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("workspace has no current version"))?;
    let version = state
        .store
        .version(version_id)
        .await
        .map_err(ApiError::store)?;
    let graph = read_version_graph(&state.data_dir, &version)
        .await
        .map_err(|err| ApiError::server_error(err.to_string()))?;
    Ok((version, graph))
}

fn count_actions(report: &MigrationReport) -> MigrationCounts {
    let mut counts = MigrationCounts::default();
    for node in &report.nodes {
        match &node.action {
            MigrationAction::Structural => counts.structural += 1,
            MigrationAction::MappedPolicy { .. } | MigrationAction::MappedPinned { .. } => {
                counts.mapped += 1;
            }
            MigrationAction::NeedsResolution { .. } | MigrationAction::NeedsUserChoice { .. } => {
                counts.needs_resolution += 1;
            }
        }
    }
    counts
}

#[cfg(test)]
#[path = "migration_routes_tests.rs"]
mod tests;
