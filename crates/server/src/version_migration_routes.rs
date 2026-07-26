use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{Path, State},
};
use helixflow_graph::semantics::{
    MIGRATION_VERSION, MigrationAction, MigrationReasonCode, migrate_v1_for_connector,
};
use helixflow_graph::{GraphService, WorkflowGraph};
use helixflow_registry::NodeRegistry;
use helixflow_store::{
    ApplyVersionMigration, NewVersionMigrationAssessment, StoreError, VersionRecord,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::catalog_routes::shared_catalog;
use crate::version_file_consistency::{
    CandidateKind, VersionFileCandidate, VersionFileConsistencyError, read_version_graph,
};
use crate::version_semantics::{
    canonical_semantics_graph, validate_legacy_migration_source, validate_migration_candidate,
};
use crate::workspace_state::workspace_state_value;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum VersionMigrationReasonCode {
    UnknownNodeType,
    CapabilityNotFound,
    ModelNotFound,
    ModelAmbiguous,
    ModelCapabilityMismatch,
    BindingNotFound,
    BindingAmbiguous,
    DefaultBindingMissing,
    SourceGraphMissing,
    SourceGraphHashMismatch,
    SourceGraphInvalid,
    SourceGraphStructuralInvalid,
    MigratedGraphInvalid,
    SemanticsInvalid,
    WorkspaceConnectorIncompatible,
}

impl From<MigrationReasonCode> for VersionMigrationReasonCode {
    fn from(value: MigrationReasonCode) -> Self {
        match value {
            MigrationReasonCode::UnknownNodeType => Self::UnknownNodeType,
            MigrationReasonCode::CapabilityNotFound => Self::CapabilityNotFound,
            MigrationReasonCode::ModelNotFound => Self::ModelNotFound,
            MigrationReasonCode::ModelAmbiguous => Self::ModelAmbiguous,
            MigrationReasonCode::ModelCapabilityMismatch => Self::ModelCapabilityMismatch,
            MigrationReasonCode::BindingNotFound => Self::BindingNotFound,
            MigrationReasonCode::BindingAmbiguous => Self::BindingAmbiguous,
            MigrationReasonCode::DefaultBindingMissing => Self::DefaultBindingMissing,
            MigrationReasonCode::WorkspaceConnectorIncompatible => {
                Self::WorkspaceConnectorIncompatible
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum VersionMigrationStatus {
    Migratable,
    NeedsResolution,
    AlreadyMigrated,
    Failed,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VersionMigrationReport {
    status: VersionMigrationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<VersionMigrationReasonCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    migration_version: String,
    workspace_id: String,
    source_version_id: String,
    source_graph_hash: String,
    source_schema_version: u32,
    catalog_revision: String,
    workspace_connector_id: String,
    apply_enabled: bool,
    report_hash: String,
    nodes: Vec<NodeMigrationResult>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct NodeMigrationResult {
    node_id: String,
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<VersionMigrationReasonCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    candidates: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ApplyVersionMigrationRequest {
    pub(crate) operation_id: String,
    pub(crate) report_hash: String,
    pub(crate) source_graph_hash: String,
    pub(crate) catalog_revision: String,
    pub(crate) workspace_connector_id: String,
    pub(crate) migration_version: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApplyVersionMigrationResponse {
    target_version_id: String,
    target_graph_hash: String,
    replayed: bool,
    workspace_state: Value,
}

struct Assessment {
    report: VersionMigrationReport,
    migrated: Option<WorkflowGraph>,
}

pub(crate) async fn dry_run_version_migration(
    Path((workspace_id, version_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<VersionMigrationReport>, ApiError> {
    let assessment = assess_current_version(&state, &workspace_id, &version_id, true).await?;
    Ok(Json(assessment.report))
}

pub(crate) async fn apply_version_migration(
    Path((workspace_id, version_id)): Path<(String, String)>,
    State(state): State<AppState>,
    Json(request): Json<ApplyVersionMigrationRequest>,
) -> Result<Json<ApplyVersionMigrationResponse>, ApiError> {
    if request.operation_id.trim().is_empty() {
        return Err(ApiError::bad_request_with_details(
            "operationId must not be empty",
            json!({"code": "OPERATION_ID_INVALID"}),
        ));
    }
    let fingerprint = operation_fingerprint(&workspace_id, &version_id, &request)?;
    if let Some(existing) = state
        .store
        .version_migration_by_operation(&workspace_id, &request.operation_id)
        .await
        .map_err(ApiError::store)?
    {
        if existing.operation_fingerprint != fingerprint {
            let report = serde_json::from_str(&existing.report_json).map_err(|error| {
                ApiError::server_error(format!("decode migration audit: {error}"))
            })?;
            persist_conflict_assessment(&state, &report, "OPERATION_ID_CONFLICT").await?;
            return Err(operation_conflict(&request.operation_id));
        }
        return Ok(Json(
            migration_response(&state, &workspace_id, &existing.target_version_id, true).await?,
        ));
    }

    if !state.migration_apply_enabled {
        return Err(ApiError::service_unavailable_with_details(
            "version migration apply is disabled",
            json!({"code": "MIGRATION_APPLY_DISABLED"}),
        ));
    }

    let assessment = assess_current_version(&state, &workspace_id, &version_id, true).await?;
    if assessment.report.workspace_connector_id != request.workspace_connector_id {
        persist_conflict_assessment(&state, &assessment.report, "WORKSPACE_CONNECTOR_STALE")
            .await?;
        return Err(ApiError::conflict_with_details(
            "workspace connector changed during migration",
            json!({"code": "WORKSPACE_CONNECTOR_STALE"}),
        ));
    }
    if let Err(error) = ensure_report_preconditions(&assessment.report, &request) {
        persist_conflict_assessment(&state, &assessment.report, "REPORT_STALE").await?;
        return Err(error);
    }
    if assessment.report.status != VersionMigrationStatus::Migratable {
        return Err(ApiError::conflict_with_details(
            "version migration report is not migratable",
            json!({"code": "MIGRATION_NOT_READY"}),
        ));
    }
    let workspace = state
        .store
        .workspace(&workspace_id)
        .await
        .map_err(ApiError::store)?;
    if state.selected_provider_for_workspace(&workspace) != assessment.report.workspace_connector_id
    {
        persist_conflict_assessment(&state, &assessment.report, "WORKSPACE_CONNECTOR_STALE")
            .await?;
        return Err(ApiError::conflict_with_details(
            "workspace connector changed during migration",
            json!({"code": "WORKSPACE_CONNECTOR_STALE"}),
        ));
    }
    let expected_runtime_provider_id = workspace.runtime_provider_id;
    let migrated = assessment.migrated.ok_or_else(|| {
        ApiError::server_error("migratable report did not produce a migration candidate")
    })?;
    let semantics_json = serde_json::to_string(&migrated.collected_semantics())
        .map_err(|error| ApiError::server_error(format!("encode migration semantics: {error}")))?;
    let report_json = serde_json::to_string(&assessment.report)
        .map_err(|error| ApiError::server_error(format!("encode migration report: {error}")))?;
    let mut candidate =
        VersionFileCandidate::from_graph(&workspace_id, CandidateKind::Migration, &migrated)
            .map_err(candidate_error)?;
    candidate
        .publish(&state.data_dir)
        .map_err(candidate_error)?;
    let target_path = candidate
        .relative_path_text()
        .map_err(candidate_error)?
        .to_owned();
    let target_hash = candidate.graph_hash().to_owned();

    let result = state
        .store
        .apply_version_migration(ApplyVersionMigration {
            workspace_id: &workspace_id,
            operation_id: &request.operation_id,
            operation_fingerprint: &fingerprint,
            source_version_id: &version_id,
            source_graph_hash: &request.source_graph_hash,
            expected_runtime_provider_id: expected_runtime_provider_id.as_deref(),
            target_label: "Migrated to graph v2",
            target_graph_path: &target_path,
            target_graph_hash: &target_hash,
            target_semantics_json: &semantics_json,
            migration_version: &request.migration_version,
            catalog_revision: &request.catalog_revision,
            workspace_connector_id: &request.workspace_connector_id,
            report_hash: &request.report_hash,
            report_json: &report_json,
        })
        .await;

    let applied = match result {
        Ok(applied) if applied.replayed => {
            candidate
                .cleanup_after_store_error(&state.store)
                .await
                .map_err(candidate_error)?;
            applied
        }
        Ok(applied) => {
            candidate.mark_committed().map_err(candidate_error)?;
            applied
        }
        Err(error) => {
            candidate
                .cleanup_after_store_error(&state.store)
                .await
                .map_err(candidate_error)?;
            let conflict_code = match &error {
                StoreError::OperationIdConflict { .. } => Some("OPERATION_ID_CONFLICT"),
                StoreError::VersionConflict { .. } => Some("CURRENT_VERSION_CONFLICT"),
                StoreError::WorkspaceConnectorConflict { .. } => Some("WORKSPACE_CONNECTOR_STALE"),
                StoreError::PendingProposalConflict { .. } => Some("PENDING_PROPOSAL"),
                _ => None,
            };
            if let Some(code) = conflict_code {
                persist_conflict_assessment(&state, &assessment.report, code).await?;
            }
            return Err(migration_store_error(error));
        }
    };
    Ok(Json(
        migration_response(
            &state,
            &workspace_id,
            &applied.target_version.id,
            applied.replayed,
        )
        .await?,
    ))
}

async fn assess_current_version(
    state: &AppState,
    workspace_id: &str,
    version_id: &str,
    persist: bool,
) -> Result<Assessment, ApiError> {
    let workspace = state
        .store
        .workspace(workspace_id)
        .await
        .map_err(ApiError::store)?;
    let version = state
        .store
        .version(version_id)
        .await
        .map_err(ApiError::store)?;
    if version.workspace_id != workspace_id {
        return Err(ApiError::not_found(
            "version was not found in this workspace",
        ));
    }
    let workspace_connector_id = state.selected_provider_for_workspace(&workspace);
    if workspace.cur_version_id.as_deref() != Some(version_id) {
        if persist {
            let report = finalize_report(VersionMigrationReport {
                status: VersionMigrationStatus::Failed,
                code: None,
                message: None,
                migration_version: MIGRATION_VERSION.to_owned(),
                workspace_id: workspace_id.to_owned(),
                source_version_id: version_id.to_owned(),
                source_graph_hash: version.graph_hash.clone(),
                source_schema_version: 0,
                catalog_revision: shared_catalog().catalog_revision.clone(),
                workspace_connector_id,
                apply_enabled: state.migration_apply_enabled,
                report_hash: String::new(),
                nodes: Vec::new(),
            })?;
            persist_conflict_assessment(state, &report, "SOURCE_VERSION_STALE").await?;
        }
        return Err(ApiError::conflict_with_details(
            "migration source is not the workspace current version",
            json!({"code": "SOURCE_VERSION_STALE"}),
        ));
    }
    let catalog = shared_catalog();
    let graph = match read_version_graph(&state.data_dir, &version).await {
        Ok(graph) => graph,
        Err(error) => {
            let (code, message) = source_file_failure(&error)?;
            let report = finalize_report(VersionMigrationReport {
                status: VersionMigrationStatus::Failed,
                code: Some(code),
                message: Some(message),
                migration_version: MIGRATION_VERSION.to_owned(),
                workspace_id: workspace_id.to_owned(),
                source_version_id: version_id.to_owned(),
                source_graph_hash: version.graph_hash.clone(),
                source_schema_version: 0,
                catalog_revision: catalog.catalog_revision.clone(),
                workspace_connector_id,
                apply_enabled: state.migration_apply_enabled,
                report_hash: String::new(),
                nodes: Vec::new(),
            })?;
            if persist {
                persist_assessment(state, &report).await?;
            }
            return Ok(Assessment {
                report,
                migrated: None,
            });
        }
    };

    let canonical = match canonical_semantics_graph(&graph, version.semantics_json.as_deref()) {
        Ok(canonical) => canonical,
        Err(_) => {
            return validation_failure_assessment(
                state,
                workspace_id,
                version_id,
                &version,
                graph.schema_version,
                workspace_connector_id,
                VersionMigrationReasonCode::SemanticsInvalid,
                persist,
            )
            .await;
        }
    };
    if let Some(canonical) = canonical {
        if validate_migration_candidate(&canonical).is_err() {
            return validation_failure_assessment(
                state,
                workspace_id,
                version_id,
                &version,
                graph.schema_version,
                workspace_connector_id,
                VersionMigrationReasonCode::SemanticsInvalid,
                persist,
            )
            .await;
        }
        let report = finalize_report(VersionMigrationReport {
            status: VersionMigrationStatus::AlreadyMigrated,
            code: None,
            message: None,
            migration_version: MIGRATION_VERSION.to_owned(),
            workspace_id: workspace_id.to_owned(),
            source_version_id: version_id.to_owned(),
            source_graph_hash: version.graph_hash.clone(),
            source_schema_version: graph.schema_version,
            catalog_revision: catalog.catalog_revision.clone(),
            workspace_connector_id,
            apply_enabled: state.migration_apply_enabled,
            report_hash: String::new(),
            nodes: Vec::new(),
        })?;
        if persist {
            persist_assessment(state, &report).await?;
        }
        return Ok(Assessment {
            report,
            migrated: None,
        });
    }

    if validate_legacy_migration_source(&graph).is_err() {
        return validation_failure_assessment(
            state,
            workspace_id,
            version_id,
            &version,
            graph.schema_version,
            workspace_connector_id,
            VersionMigrationReasonCode::SourceGraphStructuralInvalid,
            persist,
        )
        .await;
    }
    let service = GraphService::new(NodeRegistry::builtin());
    let (migrated, graph_report) =
        migrate_v1_for_connector(&graph, service.registry(), catalog, &workspace_connector_id);
    if migrated
        .as_ref()
        .is_some_and(|candidate| validate_migration_candidate(candidate).is_err())
    {
        return validation_failure_assessment(
            state,
            workspace_id,
            version_id,
            &version,
            graph.schema_version,
            workspace_connector_id,
            VersionMigrationReasonCode::MigratedGraphInvalid,
            persist,
        )
        .await;
    }
    let status = if graph_report.resolvable {
        VersionMigrationStatus::Migratable
    } else {
        VersionMigrationStatus::NeedsResolution
    };
    let nodes = graph_report
        .nodes
        .into_iter()
        .map(|node| map_node_result(node.node_id, node.action))
        .collect();
    let report = finalize_report(VersionMigrationReport {
        status,
        code: None,
        message: None,
        migration_version: graph_report.migration_version,
        workspace_id: workspace_id.to_owned(),
        source_version_id: version_id.to_owned(),
        source_graph_hash: version.graph_hash,
        source_schema_version: graph_report.source_schema_version,
        catalog_revision: graph_report.catalog_revision,
        workspace_connector_id,
        apply_enabled: state.migration_apply_enabled,
        report_hash: String::new(),
        nodes,
    })?;
    if persist {
        persist_assessment(state, &report).await?;
    }
    Ok(Assessment { report, migrated })
}

async fn validation_failure_assessment(
    state: &AppState,
    workspace_id: &str,
    version_id: &str,
    version: &VersionRecord,
    source_schema_version: u32,
    workspace_connector_id: String,
    code: VersionMigrationReasonCode,
    persist: bool,
) -> Result<Assessment, ApiError> {
    let report = semantics_failure_report(
        state,
        workspace_id,
        version_id,
        version,
        source_schema_version,
        workspace_connector_id,
        code,
    )?;
    if persist {
        persist_assessment(state, &report).await?;
    }
    Ok(Assessment {
        report,
        migrated: None,
    })
}

fn map_node_result(node_id: String, action: MigrationAction) -> NodeMigrationResult {
    match action {
        MigrationAction::Structural => node_result(node_id, "structural"),
        MigrationAction::MappedPolicy { .. } => node_result(node_id, "mapped_policy"),
        MigrationAction::MappedPinned { .. } => node_result(node_id, "mapped_pinned"),
        MigrationAction::NeedsResolution { code, reason } => NodeMigrationResult {
            node_id,
            action: "needs_resolution".to_owned(),
            code: Some(code.into()),
            message: Some(reason),
            candidates: Vec::new(),
        },
        MigrationAction::NeedsUserChoice { code, candidates } => NodeMigrationResult {
            node_id,
            action: "needs_user_choice".to_owned(),
            code: Some(code.into()),
            message: None,
            candidates,
        },
    }
}

fn node_result(node_id: String, action: &str) -> NodeMigrationResult {
    NodeMigrationResult {
        node_id,
        action: action.to_owned(),
        code: None,
        message: None,
        candidates: Vec::new(),
    }
}

fn finalize_report(mut report: VersionMigrationReport) -> Result<VersionMigrationReport, ApiError> {
    let mut value = serde_json::to_value(&report)
        .map_err(|error| ApiError::server_error(format!("encode migration report: {error}")))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| ApiError::server_error("migration report was not an object"))?;
    object.remove("reportHash");
    object.remove("applyEnabled");
    report.report_hash = hash_bytes(
        &serde_json::to_vec(&value)
            .map_err(|error| ApiError::server_error(format!("hash migration report: {error}")))?,
    );
    Ok(report)
}

async fn persist_assessment(
    state: &AppState,
    report: &VersionMigrationReport,
) -> Result<(), ApiError> {
    let mut counts = BTreeMap::<String, u64>::new();
    for code in report
        .nodes
        .iter()
        .filter_map(|node| node.code)
        .chain(report.code)
    {
        let key = serde_json::to_value(code)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or_else(|| ApiError::server_error("encode migration reason code"))?;
        *counts.entry(key).or_default() += 1;
    }
    let counts_json = serde_json::to_string(&counts)
        .map_err(|error| ApiError::server_error(format!("encode reason counts: {error}")))?;
    let top_level_code = report
        .code
        .map(|code| serde_json::to_value(code))
        .transpose()
        .map_err(|error| ApiError::server_error(format!("encode top-level code: {error}")))?
        .and_then(|value| value.as_str().map(str::to_owned));
    let status = serde_json::to_value(report.status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| ApiError::server_error("encode migration status"))?;
    state
        .store
        .record_version_migration_assessment(NewVersionMigrationAssessment {
            workspace_id: &report.workspace_id,
            source_version_id: &report.source_version_id,
            source_graph_hash: &report.source_graph_hash,
            migration_version: &report.migration_version,
            catalog_revision: &report.catalog_revision,
            workspace_connector_id: &report.workspace_connector_id,
            status: &status,
            top_level_code: top_level_code.as_deref(),
            reason_code_counts_json: &counts_json,
        })
        .await
        .map_err(ApiError::store)?;
    Ok(())
}

async fn persist_conflict_assessment(
    state: &AppState,
    report: &VersionMigrationReport,
    code: &str,
) -> Result<(), ApiError> {
    state
        .store
        .record_version_migration_assessment(NewVersionMigrationAssessment {
            workspace_id: &report.workspace_id,
            source_version_id: &report.source_version_id,
            source_graph_hash: &report.source_graph_hash,
            migration_version: &report.migration_version,
            catalog_revision: &report.catalog_revision,
            workspace_connector_id: &report.workspace_connector_id,
            status: "conflict",
            top_level_code: Some(code),
            reason_code_counts_json: "{}",
        })
        .await
        .map_err(ApiError::store)?;
    Ok(())
}

fn semantics_failure_report(
    state: &AppState,
    workspace_id: &str,
    version_id: &str,
    version: &VersionRecord,
    source_schema_version: u32,
    workspace_connector_id: String,
    code: VersionMigrationReasonCode,
) -> Result<VersionMigrationReport, ApiError> {
    finalize_report(VersionMigrationReport {
        status: VersionMigrationStatus::Failed,
        code: Some(code),
        message: Some("graph semantics or structure are invalid".to_owned()),
        migration_version: MIGRATION_VERSION.to_owned(),
        workspace_id: workspace_id.to_owned(),
        source_version_id: version_id.to_owned(),
        source_graph_hash: version.graph_hash.clone(),
        source_schema_version,
        catalog_revision: shared_catalog().catalog_revision.clone(),
        workspace_connector_id,
        apply_enabled: state.migration_apply_enabled,
        report_hash: String::new(),
        nodes: Vec::new(),
    })
}

fn source_file_failure(
    error: &VersionFileConsistencyError,
) -> Result<(VersionMigrationReasonCode, String), ApiError> {
    match error {
        VersionFileConsistencyError::MissingFile { .. } => Ok((
            VersionMigrationReasonCode::SourceGraphMissing,
            "source graph file is missing".to_owned(),
        )),
        VersionFileConsistencyError::HashMismatch { .. }
        | VersionFileConsistencyError::InvalidStoredHash { .. } => Ok((
            VersionMigrationReasonCode::SourceGraphHashMismatch,
            "source graph hash is invalid or does not match".to_owned(),
        )),
        VersionFileConsistencyError::InvalidJson { .. }
        | VersionFileConsistencyError::UnsafeRelativePath { .. } => Ok((
            VersionMigrationReasonCode::SourceGraphInvalid,
            "source graph payload is invalid".to_owned(),
        )),
        VersionFileConsistencyError::Io { .. } => Err(ApiError::server_error(
            "source graph storage could not be read",
        )),
        _ => Err(ApiError::server_error(
            "source graph validation failed unexpectedly",
        )),
    }
}

fn ensure_report_preconditions(
    report: &VersionMigrationReport,
    request: &ApplyVersionMigrationRequest,
) -> Result<(), ApiError> {
    let matches = report.report_hash == request.report_hash
        && report.source_graph_hash == request.source_graph_hash
        && report.catalog_revision == request.catalog_revision
        && report.workspace_connector_id == request.workspace_connector_id
        && report.migration_version == request.migration_version;
    if matches {
        return Ok(());
    }
    Err(ApiError::conflict_with_details(
        "migration report is stale",
        json!({"code": "REPORT_STALE"}),
    ))
}

pub(crate) fn operation_fingerprint(
    workspace_id: &str,
    version_id: &str,
    request: &ApplyVersionMigrationRequest,
) -> Result<String, ApiError> {
    let value = json!({
        "workspaceId": workspace_id,
        "sourceVersionId": version_id,
        "sourceGraphHash": request.source_graph_hash,
        "reportHash": request.report_hash,
        "catalogRevision": request.catalog_revision,
        "workspaceConnectorId": request.workspace_connector_id,
        "migrationVersion": request.migration_version,
    });
    Ok(hash_bytes(&serde_json::to_vec(&value).map_err(
        |error| ApiError::server_error(format!("encode operation: {error}")),
    )?))
}

fn hash_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("sha256:{digest:x}")
}

fn operation_conflict(operation_id: &str) -> ApiError {
    ApiError::conflict_with_details(
        format!("operation `{operation_id}` was already used with different input"),
        json!({"code": "OPERATION_ID_CONFLICT"}),
    )
}

fn migration_store_error(error: StoreError) -> ApiError {
    match error {
        StoreError::OperationIdConflict { operation_id, .. } => operation_conflict(&operation_id),
        StoreError::VersionConflict { .. } => ApiError::conflict_with_details(
            "workspace current version changed during migration",
            json!({"code": "SOURCE_VERSION_STALE"}),
        ),
        StoreError::WorkspaceConnectorConflict { .. } => ApiError::conflict_with_details(
            "workspace connector changed during migration",
            json!({"code": "WORKSPACE_CONNECTOR_STALE"}),
        ),
        StoreError::PendingProposalConflict { .. } => ApiError::conflict_with_details(
            "workspace has a pending proposal",
            json!({"code": "PENDING_PROPOSAL"}),
        ),
        other => ApiError::store(other),
    }
}

fn candidate_error(error: VersionFileConsistencyError) -> ApiError {
    ApiError::server_error(error.to_string())
}

async fn migration_response(
    state: &AppState,
    workspace_id: &str,
    target_version_id: &str,
    replayed: bool,
) -> Result<ApplyVersionMigrationResponse, ApiError> {
    let target = state
        .store
        .version(target_version_id)
        .await
        .map_err(ApiError::store)?;
    Ok(ApplyVersionMigrationResponse {
        target_version_id: target.id,
        target_graph_hash: target.graph_hash,
        replayed,
        workspace_state: workspace_state_value(state, workspace_id).await?,
    })
}
