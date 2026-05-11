-- Item J Pass 1 — outbound email queue.
--
-- Replaces the fire-and-forget `tokio::spawn` send path with a durable,
-- retryable queue. Every outbound email destined for an external channel
-- lands here first; a worker drains via SELECT FOR UPDATE SKIP LOCKED,
-- dispatches to SMTP, and updates the row's status. Crash-mid-send is
-- recoverable via the lease columns.
--
-- See ~/.claude/plans/item-j-email-rework.md for the full design.

CREATE TABLE outbound_emails (
    id              BIGSERIAL PRIMARY KEY,

    -- Routing / context
    channel_id      INTEGER NOT NULL REFERENCES channels(id),
    ticket_id       INTEGER REFERENCES tickets(id) ON DELETE SET NULL,
    -- comment_id is UNIQUE so a comment can never accidentally enqueue
    -- twice. The handler is idempotent at the enqueue boundary; this
    -- enforces it at the schema layer.
    comment_id      INTEGER UNIQUE REFERENCES comments(id) ON DELETE SET NULL,

    -- Wire content. Body kept compact: HTML and text only, no JSON
    -- copy of the original comment (already in `comments.content`).
    recipient       TEXT NOT NULL,
    subject         TEXT NOT NULL,
    body_text       TEXT NOT NULL,
    body_html       TEXT,

    -- The Message-ID we will stamp on the wire. Decided at enqueue
    -- time and PERSISTED so retries reuse it — receiving MTAs and
    -- customer MUAs dedupe on Message-ID, which is the primary
    -- defense against crash-mid-send duplicates.
    message_id      TEXT NOT NULL UNIQUE,
    in_reply_to     TEXT,
    references_list TEXT[] NOT NULL DEFAULT '{}',
    -- Extra headers (Auto-Submitted, X-Auto-Response-Suppress,
    -- List-Unsubscribe, etc.) — anything not modelled as a column.
    headers_json    JSONB NOT NULL DEFAULT '{}'::jsonb,

    -- State machine. Transitions:
    --   pending → sending → sent | failed | suppressed
    --   failed → sending (retry, attempts < max)
    --   failed → dead (attempts >= max)
    -- 'failed' is "will retry automatically"; 'dead' is "human action
    -- required". 'suppressed' is "recipient on suppression list, did
    -- not attempt send". Separate 'failed' / 'dead' so the worker's
    -- claim query can use a partial index without an attempts<max
    -- subexpression on the hot path.
    status          TEXT NOT NULL DEFAULT 'pending',
    attempts        INTEGER NOT NULL DEFAULT 0,
    last_error      TEXT,
    last_smtp_code  INTEGER,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- Crash-recovery lease. The row-lock from SELECT FOR UPDATE
    -- vanishes on connection drop; the lease tells the next worker
    -- pass "this was claimed at T, lease expires at T+5m, reclaim
    -- after that." Combined with deterministic Message-ID, this is
    -- at-least-once delivery.
    lease_token     UUID,
    lease_expires_at TIMESTAMPTZ,

    -- Timeline
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    sent_at         TIMESTAMPTZ,
    failed_at       TIMESTAMPTZ,

    -- Observability — Item S correlation_id flows through from the
    -- request that posted the originating comment.
    correlation_id  UUID,

    CONSTRAINT outbound_emails_status_chk
        CHECK (status IN ('pending', 'sending', 'sent', 'failed', 'dead', 'suppressed'))
);

-- Hot path: worker claim. Partial keeps the index tiny — only rows
-- the worker would ever scan. Lead with next_attempt_at because the
-- partial predicate already filters status to two values; this lets
-- the worker do a clean range scan for "due now."
CREATE INDEX outbound_emails_due_idx
    ON outbound_emails (next_attempt_at)
    WHERE status IN ('pending', 'failed');

-- Operator UI: per-ticket history.
CREATE INDEX outbound_emails_ticket_idx
    ON outbound_emails (ticket_id, created_at DESC);

-- Lease sweeper finds orphaned 'sending' rows whose worker died.
CREATE INDEX outbound_emails_lease_idx
    ON outbound_emails (lease_expires_at)
    WHERE status = 'sending';

-- Dashboard filtering: surface failures grouped by SMTP code.
CREATE INDEX outbound_emails_status_smtp_idx
    ON outbound_emails (status, last_smtp_code)
    WHERE status IN ('failed', 'dead');

-- LISTEN/NOTIFY trigger so the worker wakes on enqueue rather than
-- waiting for the 30s safety-net poll. Mirrors the same pattern used
-- for sync_actions in 2026-05-11-100000_sync_actions_notify_trigger.
-- Empty payload — the listener doesn't care which row triggered;
-- the drain query finds everything due via the partial index.
CREATE OR REPLACE FUNCTION outbound_emails_notify_trigger() RETURNS TRIGGER AS $$
BEGIN
    PERFORM pg_notify('outbound_emails_new', '');
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER tr_outbound_emails_notify
    AFTER INSERT ON outbound_emails
    FOR EACH ROW EXECUTE FUNCTION outbound_emails_notify_trigger();
