CREATE TABLE canvas_comment_states (
  workspace_id TEXT PRIMARY KEY,
  seq INTEGER NOT NULL CHECK (seq >= 0),
  comments_json TEXT NOT NULL,
  migration_source TEXT NOT NULL CHECK (migration_source IN ('empty', 'legacy_json')),
  updated_at TEXT NOT NULL,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
);

CREATE TABLE canvas_comment_operations (
  workspace_id TEXT NOT NULL,
  operation_id TEXT NOT NULL,
  operation_fingerprint TEXT NOT NULL,
  committed_seq INTEGER CHECK (committed_seq >= 0),
  created_at TEXT NOT NULL,
  PRIMARY KEY (workspace_id, operation_id),
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
);

CREATE INDEX idx_canvas_comment_operations_workspace_seq
  ON canvas_comment_operations(workspace_id, committed_seq);
