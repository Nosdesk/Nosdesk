-- Transactional outbox for webhook delivery. A trigger enqueues a row
-- here inside the SAME transaction that inserts the sync_action, so the
-- outbox row commits atomically with the event (no skipped deliveries)
-- and a rolled-back mutation enqueues nothing (no phantom deliveries) --
-- closing the gap a sync_id watermark would have under concurrent
-- out-of-order commits.
--
-- The webhook dispatcher claims rows with SELECT ... FOR UPDATE SKIP
-- LOCKED and deletes them after queueing, giving each event exactly-once
-- delivery across any number of app instances. The table stays
-- near-empty (drained continuously).
--
-- Global infrastructure, not workspace-scoped: drained via background_run
-- (nosdesk_admin / BYPASSRLS) across every workspace, so no RLS here.
CREATE TABLE webhook_outbox (
    sync_id      BIGINT PRIMARY KEY,
    enqueued_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- SECURITY DEFINER so the enqueue succeeds regardless of which role
-- inserted the sync_action (the RLS app role on a normal mutation, or
-- nosdesk_admin on a background job) without per-role grants on the
-- outbox table.
CREATE OR REPLACE FUNCTION webhook_outbox_enqueue() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER AS $$
BEGIN
    INSERT INTO webhook_outbox (sync_id) VALUES (NEW.sync_id);
    RETURN NEW;
END;
$$;

CREATE TRIGGER tr_sync_actions_webhook_outbox
    AFTER INSERT ON sync_actions
    FOR EACH ROW EXECUTE FUNCTION webhook_outbox_enqueue();

-- New tables/functions must self-own (the bulk re-owner loop in
-- 2026-06-03-040000_tenant_table_ownership predates them).
ALTER TABLE webhook_outbox OWNER TO nosdesk_admin;
ALTER FUNCTION webhook_outbox_enqueue() OWNER TO nosdesk_admin;
