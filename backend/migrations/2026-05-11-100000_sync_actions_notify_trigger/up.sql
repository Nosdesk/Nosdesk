-- Real-time outbox: every committed insert on `sync_actions` fires a
-- Postgres NOTIFY on the `sync_actions_new` channel. The backend's
-- sync-outbox listener (services/sync_outbox.rs) holds a dedicated
-- LISTEN connection, drains new rows since its watermark on each
-- notification, and broadcasts them to SSE clients.
--
-- The payload is intentionally empty. Postgres dedupes notifications
-- with the same (channel, payload) inside a single transaction, so a
-- multi-row write commits with one wakeup instead of N. The listener
-- doesn't need the sync_id from the payload — it always queries
-- `WHERE sync_id > watermark` to discover everything new since last
-- drain, which is robust to dropped notifications, batched commits,
-- and concurrent writers.
--
-- The trigger is on the partitioned parent. Postgres 11+ propagates
-- AFTER INSERT triggers from a partitioned table to all current and
-- future partitions automatically, so partition rotation
-- (services/partitions.rs) doesn't need to know about this trigger.

CREATE OR REPLACE FUNCTION sync_actions_notify() RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM pg_notify('sync_actions_new', '');
    RETURN NULL;
END;
$$;

CREATE TRIGGER sync_actions_notify_trigger
    AFTER INSERT ON sync_actions
    FOR EACH ROW
    EXECUTE FUNCTION sync_actions_notify();
