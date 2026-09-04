-- One force-rerun child per rejected parent. Kept separate from
-- run_failure_continuations so output-reject retries cannot be consumed by
-- failure self-heal sweeps.
CREATE TABLE run_reject_retries (
  parent_run_id TEXT PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
  child_run_id TEXT NOT NULL UNIQUE REFERENCES runs(id) ON DELETE CASCADE,
  retry_key TEXT NOT NULL UNIQUE,
  created_at TEXT NOT NULL
);
