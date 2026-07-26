use super::version_migration_routes::{
    ApplyVersionMigrationRequest, VersionMigrationReasonCode, operation_fingerprint,
};
use crate::version_semantics::derive_semantics_json;

#[test]
fn operation_fingerprint_is_deterministic_and_connector_bound() {
    let request = ApplyVersionMigrationRequest {
        operation_id: "op-1".to_owned(),
        report_hash: "sha256:report".to_owned(),
        source_graph_hash: "sha256:graph".to_owned(),
        catalog_revision: "catalog-1".to_owned(),
        workspace_connector_id: "atlas".to_owned(),
        migration_version: "1".to_owned(),
    };
    let first = operation_fingerprint("ws-1", "ver-1", &request).expect("fingerprint");
    let second = operation_fingerprint("ws-1", "ver-1", &request).expect("fingerprint");
    assert_eq!(first, second);

    let mut other_connector = request;
    other_connector.workspace_connector_id = "fal".to_owned();
    assert_ne!(
        first,
        operation_fingerprint("ws-1", "ver-1", &other_connector).expect("fingerprint")
    );
}

#[test]
fn reason_codes_serialize_as_stable_uppercase_ids() {
    assert_eq!(
        serde_json::to_string(&VersionMigrationReasonCode::SourceGraphStructuralInvalid)
            .expect("serialize"),
        "\"SOURCE_GRAPH_STRUCTURAL_INVALID\""
    );
}

#[tokio::test]
async fn dry_run_apply_and_lost_response_replay_use_server_truth() {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use helixflow_graph::{GraphNode, WorkflowGraph};
    use helixflow_run::EventBus;
    use helixflow_store::{NewVersion, Store, VersionSource};
    use serde_json::{Value, json};
    use tokio::net::TcpListener;

    use crate::app_state::AppState;
    use crate::graph_files::write_json_file;
    use crate::test_support::FailingWorkbenchAgent;

    let dir = tempfile::tempdir().expect("temp dir");
    let data_dir = dir.path().to_path_buf();
    let store = Store::open(&format!(
        "sqlite://{}",
        data_dir.join("helixflow.sqlite").display()
    ))
    .await
    .expect("store");
    let workspace = store
        .create_workspace("Migration")
        .await
        .expect("workspace");
    store
        .set_workspace_runtime_provider(&workspace.id, Some("atlas"))
        .await
        .expect("provider");
    let graph = WorkflowGraph {
        schema_version: 1,
        catalog_revision: None,
        nodes: BTreeMap::from([(
            "image".to_owned(),
            GraphNode {
                node_type: "image.generate".to_owned(),
                title: "Image".to_owned(),
                params: json!({"prompt":"test","aspect_ratio":"1:1","model":"nano banana"}),
                pos: [0.0, 0.0],
                size: None,
                semantics: None,
            },
        )]),
        edges: Vec::new(),
    };
    let graph_path = PathBuf::from("workspaces")
        .join(&workspace.id)
        .join("graphs")
        .join("legacy.json");
    let graph_hash = write_json_file(&data_dir, &graph_path, &graph, "legacy graph")
        .await
        .expect("graph");
    let source = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Legacy",
            source: VersionSource::Manual,
            graph_path: graph_path.to_string_lossy().as_ref(),
            graph_hash: &graph_hash,
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("source");
    let mut state = AppState::with_store_agent(
        EventBus::new(16),
        store.clone(),
        data_dir.clone(),
        Arc::new(FailingWorkbenchAgent),
        data_dir.join("sessions"),
    );
    state.migration_apply_enabled = true;
    let app = crate::app(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server");
    });
    let client = reqwest::Client::new();
    let endpoint = format!(
        "http://{address}/api/workspaces/{}/versions/{}/migration",
        workspace.id, source.id
    );

    let dry_run: Value = client
        .post(format!("{endpoint}/dry-run"))
        .send()
        .await
        .expect("dry-run")
        .error_for_status()
        .expect("dry-run status")
        .json()
        .await
        .expect("dry-run json");
    assert_eq!(dry_run["status"], "migratable");
    assert_eq!(dry_run["applyEnabled"], true);
    assert_eq!(dry_run["workspaceConnectorId"], "atlas");

    let connector_stale_request = json!({
        "operationId": "op-connector-stale",
        "reportHash": dry_run["reportHash"],
        "sourceGraphHash": dry_run["sourceGraphHash"],
        "catalogRevision": dry_run["catalogRevision"],
        "workspaceConnectorId": dry_run["workspaceConnectorId"],
        "migrationVersion": dry_run["migrationVersion"],
    });
    store
        .set_workspace_runtime_provider(&workspace.id, Some("fal"))
        .await
        .expect("switch provider");
    let connector_stale = client
        .post(format!("{endpoint}/apply"))
        .json(&connector_stale_request)
        .send()
        .await
        .expect("connector stale apply");
    assert_eq!(connector_stale.status(), reqwest::StatusCode::CONFLICT);
    assert_eq!(
        store
            .versions_for_workspace(&workspace.id)
            .await
            .expect("versions after connector conflict")
            .len(),
        1
    );
    let connector_conflict_code: String = sqlx::query_scalar(
        "SELECT top_level_code FROM version_migration_assessments \
         WHERE workspace_id = ? AND status = 'conflict' ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&workspace.id)
    .fetch_one(store.pool())
    .await
    .expect("connector conflict assessment");
    assert_eq!(connector_conflict_code, "WORKSPACE_CONNECTOR_STALE");
    store
        .set_workspace_runtime_provider(&workspace.id, Some("atlas"))
        .await
        .expect("restore provider");

    let stale_request = json!({
        "operationId": "op-stale-report",
        "reportHash": "sha256:stale",
        "sourceGraphHash": dry_run["sourceGraphHash"],
        "catalogRevision": dry_run["catalogRevision"],
        "workspaceConnectorId": dry_run["workspaceConnectorId"],
        "migrationVersion": dry_run["migrationVersion"],
    });
    let stale = client
        .post(format!("{endpoint}/apply"))
        .json(&stale_request)
        .send()
        .await
        .expect("stale apply");
    assert_eq!(stale.status(), reqwest::StatusCode::CONFLICT);
    let conflict_code: String = sqlx::query_scalar(
        "SELECT top_level_code FROM version_migration_assessments \
         WHERE workspace_id = ? AND status = 'conflict' ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&workspace.id)
    .fetch_one(store.pool())
    .await
    .expect("conflict assessment");
    assert_eq!(conflict_code, "REPORT_STALE");

    let request = json!({
        "operationId": "op-lost-response",
        "reportHash": dry_run["reportHash"],
        "sourceGraphHash": dry_run["sourceGraphHash"],
        "catalogRevision": dry_run["catalogRevision"],
        "workspaceConnectorId": dry_run["workspaceConnectorId"],
        "migrationVersion": dry_run["migrationVersion"],
    });
    let first: Value = client
        .post(format!("{endpoint}/apply"))
        .json(&request)
        .send()
        .await
        .expect("apply")
        .error_for_status()
        .expect("apply status")
        .json()
        .await
        .expect("apply json");
    assert_eq!(first["replayed"], false);
    let target_id = first["targetVersionId"]
        .as_str()
        .expect("target id")
        .to_owned();
    let target = store.version(&target_id).await.expect("target");
    let target_semantics = target.semantics_json.clone().expect("target semantics");
    let target_graph = crate::version_file_consistency::read_version_graph(&data_dir, &target)
        .await
        .expect("target graph");
    assert!(matches!(
        target_graph.nodes["image"]
            .semantics
            .as_ref()
            .expect("embedded semantics")
            .implementation,
        helixflow_registry::catalog::ImplementationSelection::Pinned { .. }
    ));
    assert!(
        store
            .version(&source.id)
            .await
            .expect("source")
            .semantics_json
            .is_none()
    );

    let stale_source = client
        .post(format!("{endpoint}/dry-run"))
        .send()
        .await
        .expect("stale source dry-run");
    assert_eq!(stale_source.status(), reqwest::StatusCode::CONFLICT);
    let stale_source_code: String = sqlx::query_scalar(
        "SELECT top_level_code FROM version_migration_assessments \
         WHERE workspace_id = ? AND status = 'conflict' ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&workspace.id)
    .fetch_one(store.pool())
    .await
    .expect("stale source assessment");
    assert_eq!(stale_source_code, "SOURCE_VERSION_STALE");

    sqlx::query("UPDATE versions SET semantics_json = NULL WHERE id = ?")
        .bind(&target_id)
        .execute(store.pool())
        .await
        .expect("clear derived sidecar");
    let embedded_only: Value = client
        .post(format!(
            "http://{address}/api/workspaces/{}/versions/{target_id}/migration/dry-run",
            workspace.id
        ))
        .send()
        .await
        .expect("embedded-only dry-run")
        .error_for_status()
        .expect("embedded-only status")
        .json()
        .await
        .expect("embedded-only json");
    assert_eq!(embedded_only["status"], "already_migrated");
    sqlx::query("UPDATE versions SET semantics_json = ? WHERE id = ?")
        .bind(&target_semantics)
        .bind(&target_id)
        .execute(store.pool())
        .await
        .expect("restore derived sidecar");

    let mut sidecar_only_graph = target_graph.clone();
    strip_embedded_semantics(&mut sidecar_only_graph);
    let sidecar_only_path = PathBuf::from("workspaces")
        .join(&workspace.id)
        .join("graphs")
        .join("sidecar-only.json");
    let sidecar_only_hash = write_json_file(
        &data_dir,
        &sidecar_only_path,
        &sidecar_only_graph,
        "sidecar-only graph",
    )
    .await
    .expect("sidecar-only graph");
    let sidecar_only = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Sidecar only",
            source: VersionSource::Migration,
            graph_path: sidecar_only_path.to_string_lossy().as_ref(),
            graph_hash: &sidecar_only_hash,
            parent_id: Some(&target_id),
            semantics_json: Some(&target_semantics),
        })
        .await
        .expect("sidecar-only version");

    let layout: Value = client
        .post(format!(
            "http://{address}/api/workspaces/{}/versions/layout",
            workspace.id
        ))
        .json(&json!({
            "baseVersionId": sidecar_only.id,
            "positions": [{"id": "image", "x": 12.0, "y": 34.0}],
        }))
        .send()
        .await
        .expect("layout")
        .error_for_status()
        .expect("layout status")
        .json()
        .await
        .expect("layout json");
    let layout_id = layout["workspace"]["versionId"]
        .as_str()
        .expect("layout version id")
        .to_owned();
    assert!(
        store
            .version(&layout_id)
            .await
            .expect("layout version")
            .semantics_json
            .is_some()
    );

    let ops: Value = client
        .post(format!(
            "http://{address}/api/workspaces/{}/versions/ops",
            workspace.id
        ))
        .json(&json!({
            "baseVersionId": layout_id,
            "ops": [{"op": "move_node", "id": "image", "pos": [56.0, 78.0]}],
        }))
        .send()
        .await
        .expect("ops")
        .error_for_status()
        .expect("ops status")
        .json()
        .await
        .expect("ops json");
    let ops_id = ops["workspace"]["versionId"]
        .as_str()
        .expect("ops version id")
        .to_owned();
    assert!(
        store
            .version(&ops_id)
            .await
            .expect("ops version")
            .semantics_json
            .is_some()
    );

    let restored: Value = client
        .post(format!(
            "http://{address}/api/workspaces/{}/versions/{}/restore",
            workspace.id, sidecar_only.id
        ))
        .send()
        .await
        .expect("restore")
        .error_for_status()
        .expect("restore status")
        .json()
        .await
        .expect("restore json");
    let restored_id = restored["workspace"]["versionId"]
        .as_str()
        .expect("restored version id");
    assert!(
        store
            .version(restored_id)
            .await
            .expect("restored version")
            .semantics_json
            .is_some()
    );

    let replay: Value = client
        .post(format!("{endpoint}/apply"))
        .json(&request)
        .send()
        .await
        .expect("replay")
        .error_for_status()
        .expect("replay status")
        .json()
        .await
        .expect("replay json");
    assert_eq!(replay["replayed"], true);
    assert_eq!(replay["targetVersionId"], target_id);
}

#[test]
fn derived_semantics_survive_cosmetic_edits_and_drop_deleted_nodes() {
    let (source, before) = semantic_source();
    let mut cosmetic = before.clone();
    cosmetic.nodes.get_mut("image").expect("image").title = "Renamed".to_owned();
    let encoded = derive_semantics_json(&source, &before, &cosmetic)
        .expect("cosmetic edit")
        .expect("semantics");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&encoded).expect("json"),
        serde_json::from_str::<serde_json::Value>(
            source.semantics_json.as_deref().expect("source semantics")
        )
        .expect("source json")
    );

    let mut deleted = before.clone();
    deleted.nodes.remove("image");
    let encoded = derive_semantics_json(&source, &before, &deleted)
        .expect("delete")
        .expect("semantics");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&encoded).expect("json"),
        serde_json::json!({})
    );
}

#[test]
fn derived_semantics_reject_new_executable_nodes_without_explicit_meaning() {
    let (source, before) = semantic_source();
    let mut after = before.clone();
    let mut duplicate = after.nodes["image"].clone();
    duplicate.title = "Second image".to_owned();
    duplicate.semantics = None;
    after.nodes.insert("image-2".to_owned(), duplicate);

    let error = derive_semantics_json(&source, &before, &after)
        .expect_err("new executable node must fail closed");
    assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
    assert_eq!(
        error.message,
        "derived executable graph requires valid embedded semantics"
    );
}

#[test]
fn sidecar_only_semantics_are_propagated_or_fail_closed() {
    let (source, mut before) = semantic_source();
    strip_embedded_semantics(&mut before);
    let mut cosmetic = before.clone();
    cosmetic.nodes.get_mut("image").expect("image").title = "Renamed".to_owned();
    let encoded = derive_semantics_json(&source, &before, &cosmetic)
        .expect("cosmetic edit")
        .expect("semantics");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&encoded).expect("json"),
        serde_json::from_str::<serde_json::Value>(
            source.semantics_json.as_deref().expect("source semantics")
        )
        .expect("source json")
    );

    let mut deleted = before.clone();
    deleted.nodes.remove("image");
    let error =
        derive_semantics_json(&source, &before, &deleted).expect_err("delete must fail closed");
    assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn ordinary_agent_proposal_preserves_sidecar_only_semantics() {
    use std::path::PathBuf;
    use std::sync::Arc;

    use helixflow_agent::ValidatedAgentProposal;
    use helixflow_graph::{PreparedProposal, ProposalKind, ProposalState};
    use helixflow_run::EventBus;
    use helixflow_store::{NewVersion, Store, VersionSource};

    use crate::app_state::AppState;
    use crate::graph_files::write_json_file;
    use crate::test_support::FailingWorkbenchAgent;
    use crate::workbench_message_proposals::persist_and_apply_agent_proposal;

    let dir = tempfile::tempdir().expect("temp dir");
    let data_dir = dir.path().to_path_buf();
    let store = Store::open(&format!(
        "sqlite://{}",
        data_dir.join("helixflow.sqlite").display()
    ))
    .await
    .expect("store");
    let workspace = store
        .create_workspace("V2 proposal")
        .await
        .expect("workspace");
    let (semantic_record, mut graph) = semantic_source();
    strip_embedded_semantics(&mut graph);
    let graph_path = PathBuf::from("workspaces")
        .join(&workspace.id)
        .join("graphs")
        .join("v2.json");
    let graph_hash = write_json_file(&data_dir, &graph_path, &graph, "v2 graph")
        .await
        .expect("graph");
    let source = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "V2",
            source: VersionSource::Migration,
            graph_path: graph_path.to_string_lossy().as_ref(),
            graph_hash: &graph_hash,
            parent_id: None,
            semantics_json: semantic_record.semantics_json.as_deref(),
        })
        .await
        .expect("source");
    let state = AppState::with_store_agent(
        EventBus::new(16),
        store.clone(),
        data_dir.clone(),
        Arc::new(FailingWorkbenchAgent),
        data_dir.join("sessions"),
    );
    let proposal = ValidatedAgentProposal {
        session_id: "agent-v2".to_owned(),
        agent_logs: Vec::new(),
        proposal: PreparedProposal {
            base_version_id: source.id,
            kind: ProposalKind::Modify,
            title: "Cosmetic".to_owned(),
            summary: "Keep semantics".to_owned(),
            ops: Vec::new(),
            diff_summary: Vec::new(),
            preview_graph: graph,
            state: ProposalState::Pending,
            message_id: None,
        },
    };

    persist_and_apply_agent_proposal(&state, &workspace.id, &proposal, None)
        .await
        .expect("apply proposal");
    let current_id = store
        .workspace(&workspace.id)
        .await
        .expect("workspace")
        .cur_version_id
        .expect("current");
    assert_eq!(
        store
            .version(&current_id)
            .await
            .expect("current version")
            .semantics_json,
        semantic_record.semantics_json
    );
}

fn strip_embedded_semantics(graph: &mut helixflow_graph::WorkflowGraph) {
    graph.catalog_revision = None;
    for node in graph.nodes.values_mut() {
        node.semantics = None;
    }
}

fn semantic_source() -> (
    helixflow_store::VersionRecord,
    helixflow_graph::WorkflowGraph,
) {
    use std::collections::BTreeMap;

    use helixflow_graph::semantics::migrate_v1_for_connector;
    use helixflow_graph::{GraphNode, WorkflowGraph};
    use helixflow_registry::NodeRegistry;

    let graph = WorkflowGraph {
        schema_version: 1,
        catalog_revision: None,
        nodes: BTreeMap::from([(
            "image".to_owned(),
            GraphNode {
                node_type: "image.generate".to_owned(),
                title: "Image".to_owned(),
                params: serde_json::json!({"prompt":"test","aspect_ratio":"1:1"}),
                pos: [0.0, 0.0],
                size: None,
                semantics: None,
            },
        )]),
        edges: Vec::new(),
    };
    let (migrated, report) = migrate_v1_for_connector(
        &graph,
        &NodeRegistry::builtin(),
        super::catalog_routes::shared_catalog(),
        "atlas",
    );
    assert!(report.resolvable, "report: {report:?}");
    let migrated = migrated.expect("migrated");
    let source = helixflow_store::VersionRecord {
        id: "ver-v2".to_owned(),
        workspace_id: "ws-1".to_owned(),
        idx: 1,
        label: "V2".to_owned(),
        source: "migration".to_owned(),
        graph_path: "graph.json".to_owned(),
        graph_hash: "sha256:graph".to_owned(),
        parent_id: None,
        semantics_json: Some(
            serde_json::to_string(&migrated.collected_semantics()).expect("semantics"),
        ),
        created_at: "2026-07-27T00:00:00Z".to_owned(),
    };
    (source, migrated)
}
