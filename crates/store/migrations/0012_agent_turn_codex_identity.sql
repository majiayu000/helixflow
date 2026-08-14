ALTER TABLE agent_turns ADD COLUMN codex_turn_id TEXT;

CREATE INDEX idx_agent_turns_codex_turn
  ON agent_turns(codex_turn_id);
