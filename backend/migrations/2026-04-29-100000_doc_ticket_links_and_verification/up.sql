-- Phase 1 of the docs/KB redesign:
--   1. Many-to-many doc<->ticket linkage (replaces the singular
--      documentation_pages.ticket_id pointer with a join table that
--      can express both "this doc resolved that ticket" and "this
--      doc is referenced from that ticket").
--   2. Page verification fields (verified_by/at + optional expiry
--      interval) so docs can carry a trust signal that search and
--      AI grounding can rank against.

CREATE TABLE documentation_page_tickets (
    page_id     INTEGER     NOT NULL REFERENCES documentation_pages(id) ON DELETE CASCADE,
    ticket_id   INTEGER     NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    -- 'resolves' = doc was created from / closes the ticket;
    -- 'references' = doc is relevant context but didn't originate
    -- from it. Stored as a constrained text rather than a Postgres
    -- enum so adding new link types later is a one-line CHECK
    -- update instead of an enum migration.
    link_type   VARCHAR(32) NOT NULL DEFAULT 'references'
                CHECK (link_type IN ('resolves', 'references')),
    created_by  UUID        REFERENCES users(uuid) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (page_id, ticket_id)
);

CREATE INDEX idx_doc_page_tickets_ticket_id
    ON documentation_page_tickets(ticket_id);

-- Backfill the existing singular ticket_id column as 'resolves'
-- links. The column was set when a doc was authored from a ticket,
-- which matches the 'resolves' semantics.
INSERT INTO documentation_page_tickets (page_id, ticket_id, link_type)
SELECT id, ticket_id, 'resolves'
FROM documentation_pages
WHERE ticket_id IS NOT NULL
ON CONFLICT (page_id, ticket_id) DO NOTHING;

ALTER TABLE documentation_pages
    DROP COLUMN ticket_id;

-- Verification fields. verified_at NULL means the page has never
-- been verified. verify_interval_days NULL means the verification
-- never expires (handy for evergreen reference docs); a non-null
-- interval lets the UI render a stale banner once
-- verified_at + interval has elapsed.
ALTER TABLE documentation_pages
    ADD COLUMN verified_by           UUID         REFERENCES users(uuid) ON DELETE SET NULL,
    ADD COLUMN verified_at           TIMESTAMPTZ,
    ADD COLUMN verify_interval_days  INTEGER;

-- Cheap predicate index for "show me everything that's stale right
-- now" queries (verified_at is set, interval is set, and the
-- deadline is in the past).
CREATE INDEX idx_doc_pages_verified_at
    ON documentation_pages(verified_at)
    WHERE verified_at IS NOT NULL;
