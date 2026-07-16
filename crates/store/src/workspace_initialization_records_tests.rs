use super::{
    CreateWorkspaceWithInitialVersion, ReservedWorkspaceIdentity, Store, StoreError, VersionSource,
};

async fn open_temp_store() -> (Store, tempfile::TempDir) {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    let dir = tempfile::tempdir().expect("create temp dir");
    let database_url = format!("sqlite://{}", dir.path().join("helixflow.sqlite").display());
    let options = SqliteConnectOptions::from_str(&database_url)
        .expect("parse database URL")
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("connect test pool");
    let store = Store { pool };
    store.run_migrations().await.expect("run migrations");
    (store, dir)
}

fn initial_input(
    identity: ReservedWorkspaceIdentity,
) -> CreateWorkspaceWithInitialVersion<'static> {
    CreateWorkspaceWithInitialVersion {
        identity,
        name: "Atomic workspace",
        version_label: "Initial graph",
        source: VersionSource::Manual,
        graph_path: "workspaces/atomic/graphs/initial.json",
        graph_hash: "sha256:initial",
    }
}

#[tokio::test]
async fn reserved_identity_has_no_database_effect() {
    let (store, _dir) = open_temp_store().await;
    let identity = store.reserve_workspace_identity();

    assert!(identity.workspace_id.starts_with("ws_"));
    assert!(identity.initial_version_id.starts_with("ver_"));
    assert_eq!(table_count(&store, "workspaces").await, 0);
    assert_eq!(table_count(&store, "versions").await, 0);
}

#[tokio::test]
async fn initial_workspace_and_version_commit_together() {
    let (store, _dir) = open_temp_store().await;
    let identity = store.reserve_workspace_identity();

    let initialized = store
        .create_workspace_with_initial_version(initial_input(identity.clone()))
        .await
        .expect("initialize workspace");

    assert_eq!(initialized.workspace.id, identity.workspace_id);
    assert_eq!(initialized.version.id, identity.initial_version_id);
    assert_eq!(
        initialized.workspace.cur_version_id.as_deref(),
        Some(initialized.version.id.as_str())
    );
    assert_eq!(initialized.version.idx, 1);
    assert_eq!(initialized.version.parent_id, None);
}

#[tokio::test]
async fn workspace_insert_failure_rolls_back_initialization() {
    assert_initialization_fault_rolls_back(
        r#"
        CREATE TEMP TRIGGER fail_initial_workspace_insert
        BEFORE INSERT ON workspaces
        BEGIN
          SELECT RAISE(ABORT, 'fail_initial_workspace_insert');
        END
        "#,
    )
    .await;
}

#[tokio::test]
async fn version_insert_failure_rolls_back_initialization() {
    assert_initialization_fault_rolls_back(
        r#"
        CREATE TEMP TRIGGER fail_initial_version_insert
        BEFORE INSERT ON versions
        BEGIN
          SELECT RAISE(ABORT, 'fail_initial_version_insert');
        END
        "#,
    )
    .await;
}

#[tokio::test]
async fn current_update_failure_rolls_back_initialization() {
    assert_initialization_fault_rolls_back(
        r#"
        CREATE TEMP TRIGGER fail_initial_current_update
        BEFORE UPDATE OF cur_version_id ON workspaces
        WHEN NEW.cur_version_id IS NOT NULL
        BEGIN
          SELECT RAISE(ABORT, 'fail_initial_current_update');
        END
        "#,
    )
    .await;
}

async fn assert_initialization_fault_rolls_back(trigger_sql: &str) {
    let (store, _dir) = open_temp_store().await;
    let identity = store.reserve_workspace_identity();
    sqlx::query(trigger_sql)
        .execute(store.pool())
        .await
        .expect("install fault trigger");

    let error = store
        .create_workspace_with_initial_version(initial_input(identity.clone()))
        .await
        .expect_err("fault must abort initialization");

    assert!(matches!(error, StoreError::Sqlx(_)));
    assert!(store.workspace(&identity.workspace_id).await.is_err());
    assert!(store.version(&identity.initial_version_id).await.is_err());
    assert_eq!(table_count(&store, "workspaces").await, 0);
    assert_eq!(table_count(&store, "versions").await, 0);
}

async fn table_count(store: &Store, table: &str) -> i64 {
    match table {
        "workspaces" => sqlx::query_scalar("SELECT COUNT(*) FROM workspaces")
            .fetch_one(store.pool())
            .await
            .expect("count workspaces"),
        "versions" => sqlx::query_scalar("SELECT COUNT(*) FROM versions")
            .fetch_one(store.pool())
            .await
            .expect("count versions"),
        _ => panic!("unsupported test table"),
    }
}
