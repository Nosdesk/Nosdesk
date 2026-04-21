DROP INDEX IF EXISTS idx_tickets_verification_state_pending;
ALTER TABLE tickets DROP COLUMN IF EXISTS verification_state;
