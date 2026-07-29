-- GH154: durable provider-task and restart-recovery state.

CREATE TABLE run_provider_tasks (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  run_step_id TEXT NOT NULL REFERENCES run_steps(id) ON DELETE CASCADE,
  provider TEXT NOT NULL,
  dispatch_origin TEXT NOT NULL,
  recovery_scope_fingerprint TEXT NOT NULL,
  operation_key TEXT NOT NULL,
  state TEXT NOT NULL CHECK (
    state IN (
      'dispatching',
      'active',
      'result_ready',
      'completed',
      'cancelled',
      'abandoned'
    )
  ),
  dispatch_owner_id TEXT,
  dispatch_lease_expires_at TEXT,
  dispatch_deadline_at TEXT,
  provider_task_id TEXT,
  status_url TEXT,
  result_url TEXT,
  terminal_outcome TEXT,
  result_spool_path TEXT,
  result_fingerprint TEXT,
  materialization_deadline_at TEXT,
  materialization_attempts INTEGER NOT NULL DEFAULT 0
    CHECK (materialization_attempts >= 0),
  materialization_next_retry_at TEXT,
  last_error_code TEXT,
  recovery_deadline_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  ended_at TEXT,
  UNIQUE (run_step_id),
  UNIQUE (operation_key)
);

CREATE INDEX idx_run_provider_tasks_run_state
  ON run_provider_tasks(run_id, state);
CREATE INDEX idx_run_provider_tasks_dispatch_lease
  ON run_provider_tasks(state, dispatch_lease_expires_at);
CREATE INDEX idx_run_provider_tasks_materialization
  ON run_provider_tasks(state, materialization_next_retry_at);

CREATE TABLE run_step_outputs (
  run_step_id TEXT NOT NULL REFERENCES run_steps(id) ON DELETE CASCADE,
  port TEXT NOT NULL,
  artifact_id TEXT NOT NULL REFERENCES artifacts(id) ON DELETE RESTRICT,
  created_at TEXT NOT NULL,
  PRIMARY KEY (run_step_id, port),
  UNIQUE (artifact_id)
);

CREATE TABLE run_recovery_leases (
  run_id TEXT PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
  owner_id TEXT NOT NULL,
  lease_expires_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE run_execution_intents (
  run_id TEXT PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
  plan_fingerprint TEXT NOT NULL,
  estimate_fingerprint TEXT NOT NULL,
  cost_decision TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE run_terminalization_work_items (
  run_id TEXT PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
  desired_status TEXT NOT NULL CHECK (
    desired_status IN ('failed', 'interrupted')
  ),
  state TEXT NOT NULL CHECK (state IN ('settling', 'completed')),
  error_json TEXT,
  owner_id TEXT,
  lease_expires_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  completed_at TEXT
);

CREATE TABLE run_failure_continuations (
  run_id TEXT PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,
  state TEXT NOT NULL CHECK (
    state IN ('pending', 'retry_created', 'exhausted', 'completed')
  ),
  retry_key TEXT UNIQUE,
  child_run_id TEXT UNIQUE REFERENCES runs(id) ON DELETE SET NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE artifact_publish_journal (
  operation_key TEXT PRIMARY KEY,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  run_step_id TEXT NOT NULL REFERENCES run_steps(id) ON DELETE CASCADE,
  staged_path TEXT NOT NULL,
  published_path TEXT,
  artifact_id TEXT UNIQUE REFERENCES artifacts(id) ON DELETE RESTRICT,
  content_sha256 TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('staged', 'published', 'committed')),
  owner_id TEXT NOT NULL,
  expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

ALTER TABLE cost_ledger ADD COLUMN operation_key TEXT;
CREATE UNIQUE INDEX idx_cost_ledger_operation_key
  ON cost_ledger(operation_key)
  WHERE operation_key IS NOT NULL;

CREATE UNIQUE INDEX idx_messages_recovery_risk_ref
  ON messages(ref_id)
  WHERE kind = 'run_failed' AND ref_id LIKE 'recovery-risk:%';
