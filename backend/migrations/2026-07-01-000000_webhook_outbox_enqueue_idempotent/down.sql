-- Revert to the non-idempotent enqueue.
CREATE OR REPLACE FUNCTION public.webhook_outbox_enqueue() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    AS $$
BEGIN
    INSERT INTO webhook_outbox (sync_id) VALUES (NEW.sync_id);
    RETURN NEW;
END;
$$;
