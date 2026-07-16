use super::{NewVersion, Store, StoreError, VersionSource};

async fn open_temp_store() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("helixflow.sqlite");
    let database_url = format!("sqlite://{}", db_path.display());
    let store = Store::open(&database_url).await.expect("open store");
    (store, dir)
}

#[tokio::test]
async fn creates_workspace_and_version() {
    let (store, _dir) = open_temp_store().await;

    let workspace = store
        .create_workspace("First workflow")
        .await
        .expect("create workspace");
    let version = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Initial graph",
            source: VersionSource::Manual,
            graph_path: "workspaces/ws_1/graphs/ver_1.json",
            graph_hash: "sha256:graph",
            parent_id: None,
        })
        .await
        .expect("create version");
    let updated_workspace = store.workspace(&workspace.id).await.expect("workspace");

    assert_eq!(version.workspace_id, workspace.id);
    assert_eq!(version.idx, 1);
    assert_eq!(version.source, "manual");
    assert_eq!(
        updated_workspace.cur_version_id.as_deref(),
        Some(version.id.as_str())
    );
    assert_eq!(updated_workspace.runtime_provider_id, None);
}

#[tokio::test]
async fn conditional_version_creation_rejects_stale_parent() {
    let (store, _dir) = open_temp_store().await;

    let workspace = store
        .create_workspace("First workflow")
        .await
        .expect("create workspace");
    let first = store
        .create_version(NewVersion {
            workspace_id: &workspace.id,
            label: "Initial graph",
            source: VersionSource::Manual,
            graph_path: "workspaces/ws_1/graphs/ver_1.json",
            graph_hash: "sha256:graph-1",
            parent_id: None,
        })
        .await
        .expect("create first version");
    let second = store
        .create_version_after(
            NewVersion {
                workspace_id: &workspace.id,
                label: "Second graph",
                source: VersionSource::Manual,
                graph_path: "workspaces/ws_1/graphs/ver_2.json",
                graph_hash: "sha256:graph-2",
                parent_id: Some(&first.id),
            },
            &first.id,
        )
        .await
        .expect("create second version");
    let err = store
        .create_version_after(
            NewVersion {
                workspace_id: &workspace.id,
                label: "Stale graph",
                source: VersionSource::Proposal,
                graph_path: "workspaces/ws_1/graphs/ver_stale.json",
                graph_hash: "sha256:stale",
                parent_id: Some(&first.id),
            },
            &first.id,
        )
        .await
        .expect_err("stale current version should fail");

    assert!(matches!(
        err,
        StoreError::VersionConflict {
            actual_version_id: Some(actual),
            ..
        } if actual == second.id
    ));
}
