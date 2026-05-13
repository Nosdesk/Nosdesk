DROP INDEX IF EXISTS outbound_emails_idempotency_key_uidx;

ALTER TABLE outbound_emails
    DROP COLUMN idempotency_key;

-- Down: re-tighten channel_id to NOT NULL only if every existing
-- row has a channel_id. Transactional rows (channel_id IS NULL)
-- block this rollback; operators wanting to roll back must first
-- DELETE FROM outbound_emails WHERE channel_id IS NULL.
ALTER TABLE outbound_emails
    ALTER COLUMN channel_id SET NOT NULL;
