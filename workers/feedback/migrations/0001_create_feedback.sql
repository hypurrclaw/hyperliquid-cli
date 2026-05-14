CREATE TABLE IF NOT EXISTS feedback (
  id TEXT PRIMARY KEY,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  source TEXT NOT NULL,
  version TEXT,
  scenario_json TEXT NOT NULL,
  contact TEXT,
  tags_json TEXT NOT NULL DEFAULT '[]',
  user_agent TEXT,
  cf_ray TEXT
);

CREATE INDEX IF NOT EXISTS idx_feedback_created_at ON feedback (created_at);
CREATE INDEX IF NOT EXISTS idx_feedback_source ON feedback (source);
