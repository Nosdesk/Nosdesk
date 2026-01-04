-- Remove delta tokens table
DROP TABLE IF EXISTS sync_delta_tokens;

-- Remove indexes
DROP INDEX IF EXISTS idx_groups_external_id;
DROP INDEX IF EXISTS idx_groups_external_source;
DROP INDEX IF EXISTS idx_groups_group_type;

-- Remove external sync columns from groups
ALTER TABLE groups DROP COLUMN IF EXISTS external_id;
ALTER TABLE groups DROP COLUMN IF EXISTS external_source;
ALTER TABLE groups DROP COLUMN IF EXISTS group_type;
ALTER TABLE groups DROP COLUMN IF EXISTS mail_enabled;
ALTER TABLE groups DROP COLUMN IF EXISTS security_enabled;
ALTER TABLE groups DROP COLUMN IF EXISTS last_synced_at;
ALTER TABLE groups DROP COLUMN IF EXISTS sync_enabled;
