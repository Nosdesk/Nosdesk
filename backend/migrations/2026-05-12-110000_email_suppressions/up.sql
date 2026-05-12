-- Email suppression list.
--
-- Addresses on this list are skipped by the outbound enqueue path
-- (the row is still recorded for audit, but with status='suppressed'
-- so it never enters the worker's `pending` claim set). Two ways to
-- land on the list:
--
--   1. Auto: a hard bounce (5xx SMTP code or 5.x.x enhanced status)
--      lands via the DSN linkage from J Pass 2.2a.
--   2. Manual: an admin adds an address from the suppression list
--      view (e.g. complaint-driven, GDPR right-to-be-forgotten).
--
-- Soft bounces (4xx — temporary failure) deliberately do NOT auto-
-- suppress: those are transient issues (mailbox full, recipient
-- server down) and blocking the recipient forever would mean a single
-- outage costs us a real customer.
--
-- Case-folding: the email column is stored lower-cased so the
-- enqueue check matches regardless of how the original address was
-- typed. The repository's `insert_if_absent` does the fold.
--
-- The metadata JSONB is intentionally loose so we can stash whatever
-- the upstream MTA reported without inventing a category vocabulary
-- early. `reason` is a short identifier (hard_bounce, manual, etc.)
-- the admin UI groups on; the diagnostic carries the verbose detail.

CREATE TABLE email_suppressions (
    email             TEXT        PRIMARY KEY,
    reason            TEXT        NOT NULL,
    bounce_diagnostic TEXT        NULL,
    bounce_count      INTEGER     NOT NULL DEFAULT 1,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    metadata          JSONB       NOT NULL DEFAULT '{}'::jsonb
);

-- For the admin "recently suppressed" list view; the table is small
-- enough that a simple index on created_at covers the common sort.
CREATE INDEX email_suppressions_created_idx
    ON email_suppressions (created_at DESC);
