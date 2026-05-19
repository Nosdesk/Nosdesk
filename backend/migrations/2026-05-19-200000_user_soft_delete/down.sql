DROP INDEX IF EXISTS idx_users_deleted_at_pending;
ALTER TABLE users DROP COLUMN IF EXISTS deleted_at;
