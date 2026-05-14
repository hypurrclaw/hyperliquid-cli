ALTER TABLE feedback ADD COLUMN ip_hash TEXT;

CREATE INDEX IF NOT EXISTS idx_feedback_ip_hash_created_at ON feedback (ip_hash, created_at);

CREATE TABLE IF NOT EXISTS feedback_rate_limits (
  ip_hash TEXT NOT NULL,
  window_start INTEGER NOT NULL,
  count INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (ip_hash, window_start)
);

CREATE INDEX IF NOT EXISTS idx_feedback_rate_limits_window ON feedback_rate_limits (window_start);
