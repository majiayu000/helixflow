use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Executor, Row, SqlitePool};
use uuid::Uuid;

mod failed_run_records;
mod proposal_records;
mod run_cleanup;
mod run_records;
mod sweep_records;
mod workspace_records;

pub use proposal_records::*;
pub use run_records::*;
pub use workspace_records::*;

pub fn module_name() -> &'static str {
    "store"
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Debug)]
pub enum StoreError {
    Sqlx(sqlx::Error),
    Migration(sqlx::migrate::MigrateError),
    VersionConflict {
        workspace_id: String,
        expected_version_id: String,
        actual_version_id: Option<String>,
    },
    PendingProposalConflict {
        workspace_id: String,
    },
    RunVersionMismatch {
        workspace_id: String,
        version_id: String,
        actual_workspace_id: Option<String>,
    },
    ProposalStateConflict {
        proposal_id: String,
        expected_state: String,
        actual_state: Option<String>,
    },
    ProposalWorkspaceMismatch {
        proposal_id: String,
        workspace_id: String,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlx(err) => write!(f, "sqlite store error: {err}"),
            Self::Migration(err) => write!(f, "sqlite migration error: {err}"),
            Self::VersionConflict {
                workspace_id,
                expected_version_id,
                actual_version_id,
            } => write!(
                f,
                "workspace `{workspace_id}` expected current version `{expected_version_id}` but found `{actual_version_id:?}`"
            ),
            Self::PendingProposalConflict { workspace_id } => {
                write!(f, "workspace `{workspace_id}` has a pending proposal")
            }
            Self::RunVersionMismatch {
                workspace_id,
                version_id,
                actual_workspace_id,
            } => write!(
                f,
                "run workspace `{workspace_id}` cannot use version `{version_id}` from workspace `{actual_workspace_id:?}`"
            ),
            Self::ProposalStateConflict {
                proposal_id,
                expected_state,
                actual_state,
            } => write!(
                f,
                "proposal `{proposal_id}` expected state `{expected_state}` but found `{actual_state:?}`"
            ),
            Self::ProposalWorkspaceMismatch {
                proposal_id,
                workspace_id,
            } => write!(
                f,
                "proposal `{proposal_id}` was not found in workspace `{workspace_id}`"
            ),
        }
    }
}

impl std::error::Error for StoreError {}

impl StoreError {
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Sqlx(sqlx::Error::RowNotFound))
    }
}

impl From<sqlx::Error> for StoreError {
    fn from(err: sqlx::Error) -> Self {
        Self::Sqlx(err)
    }
}

impl From<sqlx::migrate::MigrateError> for StoreError {
    fn from(err: sqlx::migrate::MigrateError) -> Self {
        Self::Migration(err)
    }
}

#[derive(Debug, Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    pub async fn open(database_url: &str) -> StoreResult<Self> {
        let pool = connect_pool(database_url).await?;
        let store = Self { pool };
        store.run_migrations().await?;
        Ok(store)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn run_migrations(&self) -> StoreResult<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }

    pub async fn create_workspace(&self, name: &str) -> StoreResult<WorkspaceRecord> {
        let id = new_id("ws");

        sqlx::query(
            r#"
            INSERT INTO workspaces (id, name, created_at, updated_at)
            VALUES (?, ?, current_timestamp, current_timestamp)
            "#,
        )
        .bind(&id)
        .bind(name)
        .execute(&self.pool)
        .await?;

        self.workspace(&id).await
    }

    pub async fn workspace(&self, workspace_id: &str) -> StoreResult<WorkspaceRecord> {
        let workspace = sqlx::query_as::<_, WorkspaceRecord>(
            r#"
            SELECT id, name, cur_version_id, runtime_provider_id, created_at, updated_at
            FROM workspaces
            WHERE id = ?
            "#,
        )
        .bind(workspace_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(workspace)
    }

    pub async fn set_workspace_runtime_provider(
        &self,
        workspace_id: &str,
        provider_id: Option<&str>,
    ) -> StoreResult<WorkspaceRecord> {
        sqlx::query(
            r#"
            UPDATE workspaces
            SET runtime_provider_id = ?, updated_at = current_timestamp
            WHERE id = ?
            "#,
        )
        .bind(provider_id)
        .bind(workspace_id)
        .execute(&self.pool)
        .await?;

        self.workspace(workspace_id).await
    }

    pub async fn create_version(&self, input: NewVersion<'_>) -> StoreResult<VersionRecord> {
        self.insert_version(input, None, false).await
    }

    pub async fn create_version_after(
        &self,
        input: NewVersion<'_>,
        expected_current_version_id: &str,
    ) -> StoreResult<VersionRecord> {
        self.insert_version(input, Some(expected_current_version_id), false)
            .await
    }

    pub async fn create_version_after_without_pending_proposal(
        &self,
        input: NewVersion<'_>,
        expected_current_version_id: &str,
    ) -> StoreResult<VersionRecord> {
        self.insert_version(input, Some(expected_current_version_id), true)
            .await
    }

    async fn insert_version(
        &self,
        input: NewVersion<'_>,
        expected_current_version_id: Option<&str>,
        reject_pending_proposal: bool,
    ) -> StoreResult<VersionRecord> {
        let version_id = new_id("ver");
        let mut tx = self.pool.begin().await?;

        if let Some(expected_version_id) = expected_current_version_id {
            let actual_version_id: Option<String> = sqlx::query_scalar(
                r#"
                SELECT cur_version_id
                FROM workspaces
                WHERE id = ?
                "#,
            )
            .bind(input.workspace_id)
            .fetch_one(&mut *tx)
            .await?;

            if actual_version_id.as_deref() != Some(expected_version_id) {
                return Err(StoreError::VersionConflict {
                    workspace_id: input.workspace_id.to_owned(),
                    expected_version_id: expected_version_id.to_owned(),
                    actual_version_id,
                });
            }
        }
        if reject_pending_proposal {
            let pending_proposal_id: Option<String> = sqlx::query_scalar(
                r#"
                SELECT id
                FROM proposals
                WHERE workspace_id = ? AND state = 'pending'
                ORDER BY created_at DESC, id DESC
                LIMIT 1
                "#,
            )
            .bind(input.workspace_id)
            .fetch_optional(&mut *tx)
            .await?;
            if pending_proposal_id.is_some() {
                return Err(StoreError::PendingProposalConflict {
                    workspace_id: input.workspace_id.to_owned(),
                });
            }
        }

        let idx: i64 = sqlx::query_scalar(
            r#"
            SELECT COALESCE(MAX(idx), 0) + 1
            FROM versions
            WHERE workspace_id = ?
            "#,
        )
        .bind(input.workspace_id)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO versions (
                id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id, created_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(&version_id)
        .bind(input.workspace_id)
        .bind(idx)
        .bind(input.label)
        .bind(input.source.as_str())
        .bind(input.graph_path)
        .bind(input.graph_hash)
        .bind(input.parent_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE workspaces
            SET cur_version_id = ?, updated_at = current_timestamp
            WHERE id = ?
            "#,
        )
        .bind(&version_id)
        .bind(input.workspace_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        self.version(&version_id).await
    }

    pub async fn version(&self, version_id: &str) -> StoreResult<VersionRecord> {
        let version = sqlx::query_as::<_, VersionRecord>(
            r#"
            SELECT id, workspace_id, idx, label, source, graph_path, graph_hash, parent_id, created_at
            FROM versions
            WHERE id = ?
            "#,
        )
        .bind(version_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(version)
    }

    pub async fn upsert_provider_status(
        &self,
        provider_id: &str,
        enabled: bool,
        status: &str,
        catalog_hash: Option<&str>,
        catalog_path: Option<&str>,
    ) -> StoreResult<ProviderRecord> {
        sqlx::query(
            r#"
            INSERT INTO providers (
                id, enabled, status, catalog_hash, catalog_path, last_checked_at
            )
            VALUES (?, ?, ?, ?, ?, current_timestamp)
            ON CONFLICT(id) DO UPDATE SET
                enabled = excluded.enabled,
                status = excluded.status,
                catalog_hash = excluded.catalog_hash,
                catalog_path = excluded.catalog_path,
                last_checked_at = current_timestamp
            "#,
        )
        .bind(provider_id)
        .bind(enabled)
        .bind(status)
        .bind(catalog_hash)
        .bind(catalog_path)
        .execute(&self.pool)
        .await?;

        self.provider_status(provider_id).await
    }

    pub async fn provider_status(&self, provider_id: &str) -> StoreResult<ProviderRecord> {
        let provider = sqlx::query_as::<_, ProviderRecord>(
            r#"
            SELECT id, enabled, status, catalog_hash, catalog_path, last_checked_at
            FROM providers
            WHERE id = ?
            "#,
        )
        .bind(provider_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(provider)
    }

    pub async fn journal_mode(&self) -> StoreResult<String> {
        let row = sqlx::query("PRAGMA journal_mode")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.try_get::<String, _>(0)?)
    }
}

async fn connect_pool(database_url: &str) -> StoreResult<SqlitePool> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    pool.execute("PRAGMA journal_mode = WAL").await?;
    pool.execute("PRAGMA busy_timeout = 5000").await?;

    Ok(pool)
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::now_v7().simple())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct WorkspaceRecord {
    pub id: String,
    pub name: String,
    pub cur_version_id: Option<String>,
    pub runtime_provider_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionSource {
    Manual,
    Proposal,
    Restore,
}

impl VersionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Proposal => "proposal",
            Self::Restore => "restore",
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewVersion<'a> {
    pub workspace_id: &'a str,
    pub label: &'a str,
    pub source: VersionSource,
    pub graph_path: &'a str,
    pub graph_hash: &'a str,
    pub parent_id: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct VersionRecord {
    pub id: String,
    pub workspace_id: String,
    pub idx: i64,
    pub label: String,
    pub source: String,
    pub graph_path: String,
    pub graph_hash: String,
    pub parent_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::FromRow)]
pub struct ProviderRecord {
    pub id: String,
    pub enabled: bool,
    pub status: String,
    pub catalog_hash: Option<String>,
    pub catalog_path: Option<String>,
    pub last_checked_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn open_temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let db_path = dir.path().join("helixflow.sqlite");
        let database_url = format!("sqlite://{}", db_path.display());
        let store = Store::open(&database_url).await.expect("open store");
        (store, dir)
    }

    #[test]
    fn reports_module_name() {
        assert_eq!(module_name(), "store");
    }

    #[test]
    fn serializes_workspace_record_boundary() {
        let workspace = WorkspaceRecord {
            id: "workspace-1".to_string(),
            name: "Demo workspace".to_string(),
            cur_version_id: Some("version-1".to_string()),
            runtime_provider_id: Some("atlas".to_string()),
            created_at: "2026-06-12T00:00:00Z".to_string(),
            updated_at: "2026-06-12T00:00:01Z".to_string(),
        };

        let encoded = serde_json::to_value(&workspace).expect("serialize workspace");

        assert_eq!(encoded["id"], "workspace-1");
        assert_eq!(encoded["name"], "Demo workspace");
        assert_eq!(encoded["cur_version_id"], "version-1");
        assert_eq!(encoded["runtime_provider_id"], "atlas");
    }

    #[tokio::test]
    async fn fresh_database_applies_migrations_and_wal_mode() {
        let (store, _dir) = open_temp_store().await;

        let workspace_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workspaces")
            .fetch_one(store.pool())
            .await
            .expect("query workspaces");

        assert_eq!(workspace_count, 0);
        assert_eq!(store.journal_mode().await.expect("journal mode"), "wal");
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
    async fn persists_workspace_runtime_provider_choice() {
        let (store, dir) = open_temp_store().await;
        let workspace = store
            .create_workspace("Provider choice")
            .await
            .expect("create workspace");

        let updated = store
            .set_workspace_runtime_provider(&workspace.id, Some("atlas"))
            .await
            .expect("set provider");
        assert_eq!(updated.runtime_provider_id.as_deref(), Some("atlas"));
        drop(store);

        let db_path = dir.path().join("helixflow.sqlite");
        let reopened = Store::open(&format!("sqlite://{}", db_path.display()))
            .await
            .expect("reopen store");
        let persisted = reopened.workspace(&workspace.id).await.expect("workspace");

        assert_eq!(persisted.runtime_provider_id.as_deref(), Some("atlas"));
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

    #[tokio::test]
    async fn run_creation_rejects_version_from_another_workspace() {
        let (store, _dir) = open_temp_store().await;

        let run_workspace = store
            .create_workspace("Run workspace")
            .await
            .expect("create run workspace");
        let version_workspace = store
            .create_workspace("Version workspace")
            .await
            .expect("create version workspace");
        let version = store
            .create_version(NewVersion {
                workspace_id: &version_workspace.id,
                label: "Other graph",
                source: VersionSource::Manual,
                graph_path: "workspaces/ws_other/graphs/ver_1.json",
                graph_hash: "sha256:other",
                parent_id: None,
            })
            .await
            .expect("create version");

        let err = store
            .create_run(NewRun {
                workspace_id: &run_workspace.id,
                version_id: &version.id,
                group_id: None,
                label: "Bad run",
                trigger: "manual",
                plan_json: None,
                estimate_json: None,
                status: "queued",
            })
            .await
            .expect_err("workspace/version mismatch should fail");

        assert!(matches!(
            err,
            StoreError::RunVersionMismatch {
                actual_workspace_id: Some(actual),
                ..
            } if actual == version_workspace.id
        ));
    }

    #[tokio::test]
    async fn provider_status_does_not_persist_secret_fields() {
        let (store, _dir) = open_temp_store().await;

        let provider = store
            .upsert_provider_status("mock", true, "healthy", Some("catalog-hash"), None)
            .await
            .expect("upsert provider");
        let columns: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT name
            FROM pragma_table_info('providers')
            "#,
        )
        .fetch_all(store.pool())
        .await
        .expect("provider columns");

        assert_eq!(provider.id, "mock");
        assert!(columns.iter().all(|column| {
            let name = column.to_lowercase();
            ![
                "secret",
                "api_key",
                "token",
                "credential",
                "password",
                "key",
            ]
            .iter()
            .any(|forbidden| name.contains(forbidden))
        }));
    }
}
