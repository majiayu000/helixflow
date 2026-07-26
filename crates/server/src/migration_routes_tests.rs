use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use helixflow_graph::{GraphNode, WorkflowGraph};
use helixflow_run::EventBus;
use helixflow_store::{NewVersion, Store, VersionSource};
use serde_json::json;

use super::*;
use crate::app_state::AppState;
use crate::graph_files::graph_hash;
use crate::test_support::FailingWorkbenchAgent;

fn v1_graph(image_params: serde_json::Value) -> WorkflowGraph {
    WorkflowGraph {
        schema_version: 1,
        nodes: BTreeMap::from([(
            "image".to_owned(),
            GraphNode {
                node_type: "image.generate".to_owned(),
                title: "Image".to_owned(),
                params: image_params,
                pos: [0.0, 0.0],
                size: None,
            },
        )]),
        edges: Vec::new(),
    }
}

async fn state_with_graph(graph: &WorkflowGraph) -> (AppState, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let data_dir = dir.path().to_path_buf();
    let database_url = format!("sqlite://{}", data_dir.join("helixflow.sqlite").display());
    let store = Store::open(&database_url).await.expect("open store");
    let workspace = store
        .create_workspace("Migration workspace")
        .await
        .expect("workspace");
    let graph_bytes = serde_json::to_vec_pretty(graph).expect("graph json");
    let stored_hash = graph_hash(&graph_bytes);
    store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "v1 graph",
            source: VersionSource::Manual,
            graph_path: "graphs/migration.json",
            graph_hash: &stored_hash,
            parent_id: None,
            semantics_json: None,
        })
        .await
        .expect("version");
    tokio::fs::create_dir_all(data_dir.join("graphs"))
        .await
        .expect("graphs dir");
    tokio::fs::write(data_dir.join("graphs/migration.json"), graph_bytes)
        .await
        .expect("write graph");
    let state = AppState::with_store_agent(
        EventBus::new(16),
        store,
        data_dir.clone(),
        Arc::new(FailingWorkbenchAgent),
        data_dir.join("sessions"),
    );
    (state, workspace.id, dir)
}

async fn version_count(state: &AppState, workspace_id: &str) -> usize {
    state
        .store
        .versions_for_workspace(workspace_id)
        .await
        .expect("versions")
        .len()
}

#[tokio::test]
async fn dry_run_is_deterministic_and_writes_nothing() {
    let graph = v1_graph(json!({ "prompt": "p", "aspect_ratio": "1:1" }));
    let (state, workspace_id, _dir) = state_with_graph(&graph).await;

    let first = migration_dry_run(Path(workspace_id.clone()), State(state.clone()))
        .await
        .expect("dry run")
        .0;
    let second = migration_dry_run(Path(workspace_id.clone()), State(state.clone()))
        .await
        .expect("dry run")
        .0;

    assert_eq!(first.status, MigrationStatus::Ready);
    assert_eq!(
        serde_json::to_string(&first).expect("json"),
        serde_json::to_string(&second).expect("json")
    );
    assert_eq!(first.counts.mapped, 1);
    assert_eq!(version_count(&state, &workspace_id).await, 1);
}

#[tokio::test]
async fn apply_creates_semantic_version_and_is_idempotent() {
    let graph = v1_graph(json!({ "prompt": "p", "aspect_ratio": "1:1" }));
    let (state, workspace_id, _dir) = state_with_graph(&graph).await;

    let applied = migration_apply(Path(workspace_id.clone()), State(state.clone()))
        .await
        .expect("apply")
        .0;
    assert_eq!(applied.status, MigrationStatus::Migrated);
    let migrated_id = applied.migrated_version_id.expect("new version id");
    assert_eq!(version_count(&state, &workspace_id).await, 2);

    // The new current version carries the semantic layer and the same topology.
    let migrated = state.store.version(&migrated_id).await.expect("version");
    assert!(migrated.semantics_json.is_some());
    let migrated_graph =
        crate::version_file_consistency::read_version_graph(&state.data_dir, &migrated)
            .await
            .expect("migrated graph");
    assert_eq!(
        migrated_graph.nodes.keys().collect::<Vec<_>>(),
        graph.nodes.keys().collect::<Vec<_>>()
    );
    assert_eq!(migrated_graph.edges, graph.edges);

    // Second apply is a no-op.
    let again = migration_apply(Path(workspace_id.clone()), State(state.clone()))
        .await
        .expect("apply again")
        .0;
    assert_eq!(again.status, MigrationStatus::AlreadyMigrated);
    assert_eq!(again.migrated_version_id, None);
    assert_eq!(version_count(&state, &workspace_id).await, 2);
}

#[tokio::test]
async fn unresolvable_nodes_block_apply_with_stable_reason() {
    // A legacy params.model that matches nothing in the catalog.
    let graph = v1_graph(json!({ "prompt": "p", "aspect_ratio": "1:1", "model": "sora ultra" }));
    let (state, workspace_id, _dir) = state_with_graph(&graph).await;

    let dry = migration_dry_run(Path(workspace_id.clone()), State(state.clone()))
        .await
        .expect("dry run")
        .0;
    assert_eq!(dry.status, MigrationStatus::NeedsResolution);
    assert_eq!(dry.counts.needs_resolution, 1);

    let err = migration_apply(Path(workspace_id.clone()), State(state.clone()))
        .await
        .expect_err("must not apply");
    assert_eq!(err.status, axum::http::StatusCode::CONFLICT);
    assert!(err.message.contains("manual resolution"));
    assert_eq!(version_count(&state, &workspace_id).await, 1);
}

#[tokio::test]
async fn legacy_model_param_migrates_to_pinned_semantics() {
    let graph = v1_graph(json!({ "prompt": "p", "aspect_ratio": "1:1", "model": "nano banana" }));
    let (state, workspace_id, _dir) = state_with_graph(&graph).await;

    let applied = migration_apply(Path(workspace_id.clone()), State(state.clone()))
        .await
        .expect("apply")
        .0;
    let migrated_id = applied.migrated_version_id.expect("new version id");
    let migrated = state.store.version(&migrated_id).await.expect("version");
    let semantics: BTreeMap<String, helixflow_graph::graph_v2::NodeSemanticsEntry> =
        serde_json::from_str(migrated.semantics_json.as_deref().expect("semantics"))
            .expect("parse semantics");
    assert!(matches!(
        semantics.get("image").expect("image entry").implementation,
        helixflow_registry::catalog::ImplementationSelection::Pinned { ref requested_model_id, .. }
            if requested_model_id == "google/nano-banana-2"
    ));
    // The legacy param moved into the semantic layer.
    let migrated_graph =
        crate::version_file_consistency::read_version_graph(&state.data_dir, &migrated)
            .await
            .expect("migrated graph");
    assert!(migrated_graph.nodes["image"].params.get("model").is_none());
}
