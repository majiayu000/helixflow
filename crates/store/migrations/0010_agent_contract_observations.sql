CREATE TABLE agent_contract_observations (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  user_message_id TEXT NOT NULL,
  session_id TEXT,
  contract_mode TEXT NOT NULL CHECK (contract_mode IN ('intent', 'legacy')),
  outcome TEXT NOT NULL CHECK (
    outcome IN ('started', 'success', 'clarify', 'error')
  ),
  reason_code TEXT,
  release_id TEXT,
  build_revision TEXT,
  started_at TEXT NOT NULL,
  completed_at TEXT,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
  FOREIGN KEY (user_message_id) REFERENCES messages(id) ON DELETE CASCADE,
  UNIQUE (user_message_id),
  CHECK (
    (
      outcome = 'started'
      AND reason_code IS NULL
      AND completed_at IS NULL
    )
    OR
    (
      outcome <> 'started'
      AND reason_code IS NOT NULL
      AND completed_at IS NOT NULL
    )
  )
);

CREATE INDEX idx_agent_contract_observations_window
  ON agent_contract_observations(
    started_at,
    release_id,
    build_revision,
    contract_mode,
    outcome
  );
