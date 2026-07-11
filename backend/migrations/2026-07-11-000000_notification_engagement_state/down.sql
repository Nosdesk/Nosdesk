DROP INDEX IF EXISTS idx_notifications_user_unseen;
ALTER TABLE notifications
    DROP COLUMN IF EXISTS snoozed_until,
    DROP COLUMN IF EXISTS archived_at,
    DROP COLUMN IF EXISTS seen_at;
