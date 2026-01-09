-- Drop indexes
DROP INDEX IF EXISTS idx_notification_rate_limits_lookup;
DROP INDEX IF EXISTS idx_notifications_entity;
DROP INDEX IF EXISTS idx_notifications_user_created;
DROP INDEX IF EXISTS idx_notifications_user_unread;
DROP INDEX IF EXISTS idx_notification_preferences_lookup;
DROP INDEX IF EXISTS idx_notification_preferences_user;

-- Drop tables in reverse order (respecting foreign keys)
DROP TABLE IF EXISTS notification_rate_limits;
DROP TABLE IF EXISTS notifications;
DROP TABLE IF EXISTS notification_preferences;
DROP TABLE IF EXISTS notification_types;
