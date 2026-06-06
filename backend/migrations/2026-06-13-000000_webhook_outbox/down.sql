DROP TRIGGER IF EXISTS tr_sync_actions_webhook_outbox ON sync_actions;
DROP FUNCTION IF EXISTS webhook_outbox_enqueue();
DROP TABLE IF EXISTS webhook_outbox;
