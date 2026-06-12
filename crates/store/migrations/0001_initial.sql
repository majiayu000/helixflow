CREATE TABLE workspaces (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  cur_version_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE versions (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  idx INTEGER NOT NULL,
  label TEXT NOT NULL,
  source TEXT NOT NULL CHECK (source IN ('manual', 'proposal', 'restore')),
  graph_path TEXT NOT NULL,
  graph_hash TEXT NOT NULL,
  parent_id TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
  FOREIGN KEY (parent_id) REFERENCES versions(id) ON DELETE SET NULL,
  UNIQUE (workspace_id, idx)
);

CREATE INDEX idx_versions_workspace_id ON versions(workspace_id);

CREATE TABLE proposals (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  base_version_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  title TEXT NOT NULL,
  summary TEXT NOT NULL,
  ops_path TEXT NOT NULL,
  preview_graph_path TEXT,
  state TEXT NOT NULL CHECK (state IN ('pending', 'applied', 'dismissed', 'superseded', 'failed')),
  result_version_id TEXT,
  message_id TEXT,
  created_at TEXT NOT NULL,
  resolved_at TEXT,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
  FOREIGN KEY (base_version_id) REFERENCES versions(id) ON DELETE RESTRICT,
  FOREIGN KEY (result_version_id) REFERENCES versions(id) ON DELETE SET NULL
);

CREATE INDEX idx_proposals_workspace_state ON proposals(workspace_id, state);

CREATE TABLE messages (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('user', 'agent', 'system')),
  text TEXT,
  kind TEXT NOT NULL,
  ref_id TEXT,
  attachment_ids_json TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
);

CREATE INDEX idx_messages_workspace_created ON messages(workspace_id, created_at);

CREATE TABLE runs (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  version_id TEXT NOT NULL,
  group_id TEXT,
  label TEXT NOT NULL,
  trigger TEXT NOT NULL CHECK (trigger IN ('manual', 'agent', 'sweep')),
  plan_json TEXT,
  estimate_json TEXT,
  status TEXT NOT NULL CHECK (
    status IN (
      'queued',
      'estimating',
      'waiting_confirmation',
      'running',
      'succeeded',
      'failed',
      'interrupted'
    )
  ),
  error_json TEXT,
  started_at TEXT,
  ended_at TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
  FOREIGN KEY (version_id) REFERENCES versions(id) ON DELETE RESTRICT
);

CREATE INDEX idx_runs_workspace_created ON runs(workspace_id, created_at);
CREATE INDEX idx_runs_version_id ON runs(version_id);

CREATE TABLE run_steps (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  node_id TEXT NOT NULL,
  node_type TEXT NOT NULL,
  provider TEXT,
  state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'succeeded', 'failed', 'skipped')),
  progress REAL,
  cost_estimate_json TEXT,
  cost_actual_json TEXT,
  error_json TEXT,
  started_at TEXT,
  ended_at TEXT,
  FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE
);

CREATE INDEX idx_run_steps_run_id ON run_steps(run_id);

CREATE TABLE run_events (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  ev TEXT NOT NULL,
  data_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE,
  UNIQUE (run_id, seq)
);

CREATE INDEX idx_run_events_run_seq ON run_events(run_id, seq);

CREATE TABLE uploads (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  filename TEXT NOT NULL,
  file_path TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  mime TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
);

CREATE INDEX idx_uploads_workspace_id ON uploads(workspace_id);

CREATE TABLE artifacts (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  run_id TEXT,
  run_step_id TEXT,
  node_id TEXT,
  kind TEXT NOT NULL,
  storage_uri TEXT NOT NULL,
  sha256 TEXT,
  mime TEXT,
  width INTEGER,
  height INTEGER,
  duration_ms INTEGER,
  selected INTEGER NOT NULL DEFAULT 0 CHECK (selected IN (0, 1)),
  meta_json TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
  FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE SET NULL,
  FOREIGN KEY (run_step_id) REFERENCES run_steps(id) ON DELETE SET NULL
);

CREATE INDEX idx_artifacts_workspace_created ON artifacts(workspace_id, created_at);
CREATE INDEX idx_artifacts_run_id ON artifacts(run_id);

CREATE TABLE providers (
  id TEXT PRIMARY KEY,
  enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
  status TEXT NOT NULL,
  catalog_hash TEXT,
  catalog_path TEXT,
  last_checked_at TEXT
);

CREATE TABLE cost_ledger (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  run_id TEXT,
  run_step_id TEXT,
  provider TEXT NOT NULL,
  amount REAL NOT NULL CHECK (amount >= 0),
  currency TEXT NOT NULL,
  estimated INTEGER NOT NULL CHECK (estimated IN (0, 1)),
  created_at TEXT NOT NULL,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
  FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE SET NULL,
  FOREIGN KEY (run_step_id) REFERENCES run_steps(id) ON DELETE SET NULL
);

CREATE INDEX idx_cost_ledger_workspace_created ON cost_ledger(workspace_id, created_at);
CREATE INDEX idx_cost_ledger_run_id ON cost_ledger(run_id);
