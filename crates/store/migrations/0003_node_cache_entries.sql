ALTER TABLE run_steps ADD COLUMN metadata_json TEXT;

CREATE TABLE node_cache_entries (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  provider TEXT NOT NULL,
  node_type TEXT NOT NULL,
  node_id TEXT NOT NULL,
  cache_key TEXT NOT NULL,
  input_hash_json TEXT NOT NULL,
  artifact_ids_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  last_hit_at TEXT,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
  UNIQUE (workspace_id, provider, node_type, node_id, cache_key)
);

CREATE INDEX idx_node_cache_lookup
  ON node_cache_entries(workspace_id, provider, node_type, node_id, cache_key);
