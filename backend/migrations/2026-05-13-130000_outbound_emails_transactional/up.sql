-- Generalise the outbound queue to carry transactional sends
-- (password reset, invitation, notification) alongside the
-- existing channel-mediated ticket replies.
--
-- Two changes:
--
--   1. `channel_id` becomes nullable. Channel-reply rows still
--      bind to a channel; transactional rows leave it NULL. The
--      worker treats NULL as "no channel_messages row to record
--      after send" — there's nothing to thread back into.
--
--   2. New `idempotency_key` column with a partial unique index.
--      Callers that want at-least-once → effectively-once
--      semantics set the key; the index makes a re-enqueue with
--      the same key a no-op rather than producing a duplicate
--      send. NULL keys are unconstrained so legacy channel-reply
--      rows (deduped at the handler layer via Message-ID)
--      continue to enqueue without per-row keys.
--
-- The partial index is `WHERE idempotency_key IS NOT NULL` so we
-- don't burn an index slot indexing every NULL row in the table.

ALTER TABLE outbound_emails
    ALTER COLUMN channel_id DROP NOT NULL;

ALTER TABLE outbound_emails
    ADD COLUMN idempotency_key TEXT;

CREATE UNIQUE INDEX outbound_emails_idempotency_key_uidx
    ON outbound_emails (idempotency_key)
    WHERE idempotency_key IS NOT NULL;
