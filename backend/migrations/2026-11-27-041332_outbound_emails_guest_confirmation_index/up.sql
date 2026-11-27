-- The request form's confirmations a workspace queued, by when: the daily
-- ceiling on addresses it has never mailed counts today's, and without this
-- the count reads the whole queue, which nothing prunes.
CREATE INDEX IF NOT EXISTS outbound_emails_guest_confirmation_idx
    ON public.outbound_emails (workspace_id, created_at)
    WHERE idempotency_key LIKE 'guest-confirmation:%';
