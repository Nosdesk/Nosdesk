-- Reverse session token overhaul

-- Remove new indexes and columns from refresh_tokens
DROP INDEX IF EXISTS idx_refresh_tokens_session_id;
DROP INDEX IF EXISTS idx_refresh_tokens_family_id;
ALTER TABLE refresh_tokens
  DROP COLUMN IF EXISTS grace_expires_at,
  DROP COLUMN IF EXISTS replaced_by_hash,
  DROP COLUMN IF EXISTS used_at,
  DROP COLUMN IF EXISTS is_used,
  DROP COLUMN IF EXISTS family_id,
  DROP COLUMN IF EXISTS session_id;

-- Re-add session_token column to active_sessions
ALTER TABLE active_sessions ADD COLUMN session_token VARCHAR(64) NOT NULL DEFAULT '';
CREATE INDEX idx_active_sessions_session_token ON active_sessions(session_token);

-- Remove session_id UUID column
DROP INDEX IF EXISTS idx_active_sessions_session_id;
ALTER TABLE active_sessions DROP COLUMN IF EXISTS session_id;
