-- Session & Refresh Token Architecture Overhaul
-- Adds stable session_id UUID to active_sessions, removes fragile session_token,
-- adds token family tracking and reuse detection to refresh_tokens.

-- 1. Add stable session_id UUID to active_sessions
ALTER TABLE active_sessions ADD COLUMN session_id UUID NOT NULL DEFAULT gen_random_uuid() UNIQUE;
CREATE INDEX idx_active_sessions_session_id ON active_sessions(session_id);

-- Remove session_token column (replaced by session_id UUID)
DROP INDEX IF EXISTS idx_active_sessions_session_token;
ALTER TABLE active_sessions DROP COLUMN session_token;

-- 2. Add session link, family tracking, reuse detection, grace period to refresh_tokens
ALTER TABLE refresh_tokens
  ADD COLUMN session_id UUID REFERENCES active_sessions(session_id) ON DELETE CASCADE,
  ADD COLUMN family_id UUID NOT NULL DEFAULT gen_random_uuid(),
  ADD COLUMN is_used BOOLEAN NOT NULL DEFAULT FALSE,
  ADD COLUMN used_at TIMESTAMPTZ,
  ADD COLUMN replaced_by_hash VARCHAR(64),
  ADD COLUMN grace_expires_at TIMESTAMPTZ;

CREATE INDEX idx_refresh_tokens_family_id ON refresh_tokens(family_id);
CREATE INDEX idx_refresh_tokens_session_id ON refresh_tokens(session_id);
