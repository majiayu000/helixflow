-- GH101: run failure self-repair + output review.

-- Run retry lineage: failed runs derive a new run so the failed run's
-- error_json is preserved for audit (never overwritten).
ALTER TABLE runs ADD COLUMN parent_run_id TEXT REFERENCES runs(id) ON DELETE SET NULL;
ALTER TABLE runs ADD COLUMN attempt INTEGER NOT NULL DEFAULT 0;
ALTER TABLE runs ADD COLUMN force_rerun INTEGER NOT NULL DEFAULT 0;
CREATE INDEX idx_runs_parent_run_id ON runs(parent_run_id);

-- Artifact review state: outputs start pending and require accept/reject.
ALTER TABLE artifacts ADD COLUMN review_state TEXT NOT NULL DEFAULT 'pending'
  CHECK (review_state IN ('pending', 'accepted', 'rejected'));
-- Backfill: artifacts created before review existed are treated as accepted.
UPDATE artifacts SET review_state = 'accepted';
