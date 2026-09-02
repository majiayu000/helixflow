CREATE TABLE canvas_snapshots (
  workspace_id TEXT PRIMARY KEY,
  revision INTEGER NOT NULL CHECK (revision >= 0),
  nodes_json TEXT NOT NULL,
  viewport_json TEXT,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
);
