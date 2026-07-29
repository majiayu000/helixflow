-- GH153: durable bounded Agent graph-repair state.

CREATE TABLE run_repair_chains (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  root_run_id TEXT NOT NULL UNIQUE REFERENCES runs(id) ON DELETE CASCADE,
  provenance TEXT NOT NULL CHECK (provenance IN ('agent', 'recommended_sweep')),
  sweep_group_id TEXT,
  recommended_run_id TEXT REFERENCES runs(id) ON DELETE CASCADE,
  created_at TEXT NOT NULL
);

CREATE TABLE run_repair_chain_runs (
  run_id TEXT PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
  chain_id TEXT NOT NULL REFERENCES run_repair_chains(id) ON DELETE CASCADE,
  relation TEXT NOT NULL CHECK (relation IN ('root', 'retry', 'fix_child')),
  created_at TEXT NOT NULL
);

ALTER TABLE run_failure_continuations RENAME TO run_failure_continuations_gh154;

CREATE TABLE run_failure_continuations (
  run_id TEXT PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
  state TEXT NOT NULL CHECK (
    state IN (
      'pending',
      'retry_created',
      'exhausted',
      'completed',
      'fix_pending',
      'fix_claimed',
      'fix_completed'
    )
  ),
  retry_key TEXT UNIQUE,
  child_run_id TEXT UNIQUE REFERENCES runs(id) ON DELETE SET NULL,
  chain_id TEXT REFERENCES run_repair_chains(id) ON DELETE SET NULL,
  reason_code TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

INSERT INTO run_failure_continuations (
  run_id, state, retry_key, child_run_id, chain_id, reason_code, created_at, updated_at
)
SELECT run_id, state, retry_key, child_run_id, NULL, NULL, created_at, updated_at
FROM run_failure_continuations_gh154;

DROP TABLE run_failure_continuations_gh154;

CREATE TABLE run_fix_attempts (
  id TEXT PRIMARY KEY,
  operation_id TEXT NOT NULL UNIQUE,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  chain_id TEXT NOT NULL REFERENCES run_repair_chains(id) ON DELETE CASCADE,
  root_run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  source_run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  attempt_index INTEGER NOT NULL CHECK (attempt_index > 0),
  max_attempts INTEGER NOT NULL CHECK (max_attempts >= 0),
  source_version_id TEXT NOT NULL REFERENCES versions(id) ON DELETE CASCADE,
  expected_runtime_provider_id TEXT,
  effective_provider_id TEXT NOT NULL,
  expected_recovery_scope_fingerprint TEXT NOT NULL,
  provider_catalog_fingerprint TEXT NOT NULL,
  state TEXT NOT NULL CHECK (
    state IN (
      'claimed',
      'agent_running',
      'version_applied',
      'child_preparing',
      'child_ready',
      'failed',
      'exhausted',
      'cancelled'
    )
  ),
  proposal_id TEXT REFERENCES proposals(id) ON DELETE SET NULL,
  target_version_id TEXT REFERENCES versions(id) ON DELETE SET NULL,
  child_run_id TEXT UNIQUE REFERENCES runs(id) ON DELETE SET NULL,
  reason_code TEXT,
  error_summary TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  finished_at TEXT,
  UNIQUE (root_run_id, attempt_index)
);

CREATE INDEX idx_run_fix_attempts_chain_state
  ON run_fix_attempts(chain_id, state);
CREATE INDEX idx_run_fix_attempts_source
  ON run_fix_attempts(source_run_id, attempt_index);

CREATE TABLE run_fix_event_outbox (
  id TEXT PRIMARY KEY,
  dedupe_key TEXT NOT NULL UNIQUE,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  event_name TEXT NOT NULL CHECK (
    event_name IN ('run.fix_attempt', 'run.fix_applied', 'run.fix_exhausted')
  ),
  data_json TEXT NOT NULL,
  state TEXT NOT NULL CHECK (
    state IN ('pending', 'event_persisted', 'broadcasted')
  ),
  event_id TEXT UNIQUE REFERENCES run_events(id) ON DELETE SET NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX idx_run_fix_outbox_state
  ON run_fix_event_outbox(state, created_at);
