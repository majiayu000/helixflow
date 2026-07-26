-- SQLx SQLite always runs migration files inside a transaction, where
-- PRAGMA foreign_keys cannot be disabled. Store::run_migrations therefore
-- performs the required idempotent versions table rebuild immediately after
-- this tracked migration, on one exclusive connection, before Store::open
-- returns to callers.

CREATE TABLE version_migrations (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  operation_id TEXT NOT NULL,
  operation_fingerprint TEXT NOT NULL,
  source_version_id TEXT NOT NULL,
  source_graph_hash TEXT NOT NULL,
  target_version_id TEXT NOT NULL,
  target_graph_hash TEXT NOT NULL,
  migration_version TEXT NOT NULL,
  catalog_revision TEXT NOT NULL,
  workspace_connector_id TEXT NOT NULL,
  report_hash TEXT NOT NULL,
  report_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
  FOREIGN KEY (source_version_id) REFERENCES versions(id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (target_version_id) REFERENCES versions(id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  UNIQUE (workspace_id, operation_id)
);

CREATE INDEX idx_version_migrations_workspace_created
  ON version_migrations(workspace_id, created_at);

CREATE TABLE version_migration_assessments (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  source_version_id TEXT NOT NULL,
  source_graph_hash TEXT NOT NULL,
  migration_version TEXT NOT NULL,
  catalog_revision TEXT NOT NULL,
  workspace_connector_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (
    status IN (
      'migratable',
      'needs_resolution',
      'already_migrated',
      'failed',
      'conflict'
    )
  ),
  top_level_code TEXT,
  reason_code_counts_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
  FOREIGN KEY (source_version_id) REFERENCES versions(id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
);

CREATE INDEX idx_version_migration_assessments_workspace_created
  ON version_migration_assessments(workspace_id, created_at);
