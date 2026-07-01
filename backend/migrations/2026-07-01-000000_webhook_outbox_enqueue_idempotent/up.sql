-- Make the sync_actions -> webhook_outbox enqueue idempotent.
--
-- tr_sync_actions_webhook_outbox fires AFTER INSERT on sync_actions and inserts
-- one webhook_outbox row keyed by sync_id (the outbox PK). sync_actions is
-- append-only in production, but Postgres implements an occurred_at change (the
-- monthly partition key) as DELETE + INSERT, which re-fires the trigger for the
-- same sync_id. A re-fire must not raise a duplicate-key error or enqueue a
-- second delivery, so keep exactly one outbox row per sync_id.
CREATE OR REPLACE FUNCTION public.webhook_outbox_enqueue() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    AS $$
BEGIN
    INSERT INTO webhook_outbox (sync_id) VALUES (NEW.sync_id)
    ON CONFLICT (sync_id) DO NOTHING;
    RETURN NEW;
END;
$$;
