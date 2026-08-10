CREATE TABLE conversations (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  title TEXT NOT NULL,
  codex_thread_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
);

CREATE INDEX idx_conversations_workspace_updated
  ON conversations(workspace_id, updated_at DESC, id DESC);

CREATE TABLE agent_turns (
  id TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL,
  workspace_id TEXT NOT NULL,
  user_message_id TEXT,
  execution_id TEXT,
  mode TEXT NOT NULL,
  status TEXT NOT NULL CHECK (
    status IN ('running', 'succeeded', 'clarify', 'error', 'interrupted')
  ),
  reason_code TEXT,
  started_at TEXT NOT NULL,
  completed_at TEXT,
  FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
  FOREIGN KEY (user_message_id) REFERENCES messages(id) ON DELETE SET NULL,
  CHECK (
    (status = 'running' AND completed_at IS NULL)
    OR (status <> 'running' AND completed_at IS NOT NULL)
  )
);

CREATE INDEX idx_agent_turns_conversation_started
  ON agent_turns(conversation_id, started_at, id);

ALTER TABLE messages ADD COLUMN conversation_id TEXT
  REFERENCES conversations(id) ON DELETE CASCADE;
ALTER TABLE messages ADD COLUMN turn_id TEXT
  REFERENCES agent_turns(id) ON DELETE SET NULL;

CREATE INDEX idx_messages_conversation_created
  ON messages(conversation_id, created_at, id);
CREATE INDEX idx_messages_turn_created
  ON messages(turn_id, created_at, id);

-- Existing workspaces receive one durable recovery conversation. This keeps
-- old history visible while separating new conversations from the workspace.
INSERT INTO conversations (id, workspace_id, title, created_at, updated_at)
SELECT 'conv_' || substr(id, 4), id, '历史对话', created_at, updated_at
FROM workspaces;

UPDATE messages
SET conversation_id = 'conv_' || substr(workspace_id, 4)
WHERE conversation_id IS NULL;

-- Graph-edit observations already contain a durable terminal outcome. Promote
-- them to turns so old log-only failures no longer look in-flight after reload.
INSERT INTO agent_turns (
  id, conversation_id, workspace_id, user_message_id, execution_id, mode,
  status, reason_code, started_at, completed_at
)
SELECT
  'turn_' || substr(id, 5),
  'conv_' || substr(workspace_id, 4),
  workspace_id,
  user_message_id,
  session_id,
  'graph_edit',
  CASE outcome
    WHEN 'started' THEN 'interrupted'
    WHEN 'success' THEN 'succeeded'
    WHEN 'clarify' THEN 'clarify'
    ELSE 'error'
  END,
  CASE WHEN outcome = 'started' THEN 'PROCESS_INTERRUPTED' ELSE reason_code END,
  started_at,
  COALESCE(completed_at, current_timestamp)
FROM agent_contract_observations;

UPDATE messages
SET turn_id = (
  SELECT 'turn_' || substr(observation.id, 5)
  FROM agent_contract_observations AS observation
  WHERE observation.user_message_id = messages.id
)
WHERE EXISTS (
  SELECT 1 FROM agent_contract_observations AS observation
  WHERE observation.user_message_id = messages.id
);

UPDATE messages
SET turn_id = (
  SELECT 'turn_' || substr(observation.id, 5)
  FROM agent_contract_observations AS observation
  WHERE observation.session_id = messages.ref_id
    AND observation.workspace_id = messages.workspace_id
  LIMIT 1
)
WHERE role = 'agent'
  AND ref_id IS NOT NULL
  AND EXISTS (
    SELECT 1 FROM agent_contract_observations AS observation
    WHERE observation.session_id = messages.ref_id
      AND observation.workspace_id = messages.workspace_id
  );
