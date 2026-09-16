CREATE TABLE image_processing_jobs (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  source_node_id TEXT NOT NULL,
  result_node_id TEXT,
  intent TEXT NOT NULL CHECK (
    intent IN ('outpaint', 'inpaint', 'cutout', 'upscale', 'enhance')
  ),
  profile TEXT,
  provider_task_id TEXT,
  provider TEXT,
  model TEXT,
  output_upload_id TEXT,
  status TEXT NOT NULL CHECK (
    status IN ('queued', 'running', 'succeeded', 'failed', 'interrupted')
  ),
  error TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  completed_at TEXT,
  FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE,
  FOREIGN KEY (output_upload_id) REFERENCES uploads(id) ON DELETE SET NULL
);

CREATE INDEX idx_image_processing_jobs_workspace_created
  ON image_processing_jobs(workspace_id, created_at DESC);

CREATE INDEX idx_image_processing_jobs_provider_task
  ON image_processing_jobs(provider_task_id);
