-- Merge bookkeeping on the source ticket. A merged source points at its
-- canonical destination via merged_into_ticket_id and records who
-- merged it and when. merge_reason is the optional free-text note from
-- the merge dialog.
ALTER TABLE tickets
    ADD COLUMN merged_into_ticket_id INT4 NULL
        REFERENCES tickets(id) ON DELETE SET NULL,
    ADD COLUMN merged_at TIMESTAMPTZ NULL,
    ADD COLUMN merged_by_user_uuid UUID NULL
        REFERENCES users(uuid) ON DELETE SET NULL,
    ADD COLUMN merge_reason TEXT NULL;

-- Partial index: only merged sources carry a destination, so the index
-- stays small and serves the "what was merged into ticket N" lookup.
CREATE INDEX tickets_merged_into_idx ON tickets (merged_into_ticket_id)
    WHERE merged_into_ticket_id IS NOT NULL;

-- Invariant: a row is either fully unmerged or fully merged. The three
-- bookkeeping columns move together. merge_reason is optional and not
-- part of the invariant.
ALTER TABLE tickets ADD CONSTRAINT tickets_merge_complete CHECK (
    (merged_into_ticket_id IS NULL AND merged_at IS NULL AND merged_by_user_uuid IS NULL)
    OR
    (merged_into_ticket_id IS NOT NULL AND merged_at IS NOT NULL AND merged_by_user_uuid IS NOT NULL)
);

-- Chain prevention (merging a ticket into an already-merged target) is
-- enforced in the merge handler's pre-flight, not here: Postgres CHECK
-- constraints cannot contain subqueries. See docs/ticket-merge-plan.md
-- section 4.2 and the handler integration test.
