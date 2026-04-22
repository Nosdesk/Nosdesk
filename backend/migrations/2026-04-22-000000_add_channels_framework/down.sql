DROP INDEX IF EXISTS idx_comments_ticket_id_not_deleted;
ALTER TABLE comments DROP COLUMN IF EXISTS deleted_at;
ALTER TABLE comments DROP COLUMN IF EXISTS is_internal;
ALTER TABLE comments DROP COLUMN IF EXISTS channel_metadata;

DROP INDEX IF EXISTS idx_tickets_origin_channel_id;
ALTER TABLE tickets DROP COLUMN IF EXISTS origin_channel_id;

DROP INDEX IF EXISTS idx_channel_messages_ticket_id;
DROP INDEX IF EXISTS idx_channel_messages_external_id;
DROP TABLE IF EXISTS channel_messages;

DROP TABLE IF EXISTS channel_credentials;

DROP INDEX IF EXISTS idx_channels_enabled_provider;
DROP TABLE IF EXISTS channels;
