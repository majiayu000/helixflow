CREATE TABLE canvases (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  title TEXT NOT NULL,
  seq INTEGER NOT NULL DEFAULT 0,
  snapshot_path TEXT NOT NULL,
  snapshot_hash TEXT NOT NULL,
  current_version_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
  FOREIGN KEY (current_version_id) REFERENCES versions(id) ON DELETE SET NULL
);

CREATE INDEX idx_canvases_workspace_id ON canvases(workspace_id);

CREATE TABLE canvas_ops (
  id TEXT PRIMARY KEY,
  canvas_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  base_seq INTEGER NOT NULL,
  actor_json TEXT NOT NULL,
  kind TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (canvas_id) REFERENCES canvases(id) ON DELETE CASCADE,
  UNIQUE (canvas_id, seq),
  UNIQUE (canvas_id, idempotency_key)
);

CREATE INDEX idx_canvas_ops_canvas_seq ON canvas_ops(canvas_id, seq);

CREATE TABLE canvas_presence (
  canvas_id TEXT NOT NULL,
  actor_id TEXT NOT NULL,
  cursor_json TEXT,
  selection_json TEXT,
  viewport_json TEXT,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (canvas_id, actor_id),
  FOREIGN KEY (canvas_id) REFERENCES canvases(id) ON DELETE CASCADE
);
