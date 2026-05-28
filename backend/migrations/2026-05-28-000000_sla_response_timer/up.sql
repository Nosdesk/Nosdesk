-- SLA Phase 1b — response timer source-of-truth.
--
-- The SLA engine has always carried a `target_response_minutes`
-- column on `sla_policies` but never had anywhere to compare it
-- against. This adds the missing piece: the wall-clock moment of the
-- first non-internal staff comment on a ticket. The comment-create
-- path stamps this idempotently (UPDATE ... WHERE first_response_at
-- IS NULL) so concurrent first replies don't race.
--
-- Indexed for the breach-detection scan that pairs with the
-- materialised `sla_target_at` columns (Phase 1c). The partial
-- predicate keeps the index tiny in the typical case where most
-- closed tickets already have a response stamped.

ALTER TABLE tickets
    ADD COLUMN first_response_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS tickets_first_response_at_idx
    ON tickets (first_response_at)
    WHERE first_response_at IS NULL;
