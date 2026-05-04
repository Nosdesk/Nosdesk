-- RRULE recurring tasks (Phase 7 deferred). The pattern is the
-- spec'd "materialise-on-edit": closing a recurring ticket spawns
-- the next occurrence rather than a background job ticking forever.
--
-- Two columns:
--   - recurrence_rule: RFC 5545 RRULE string
--     ("FREQ=WEEKLY;BYDAY=MO"). NULL = not recurring.
--   - recurrence_template_id: when an occurrence is spawned, this
--     points at the ticket whose recurrence_rule was the source.
--     The first ticket in a series is its own template (NULL); each
--     spawned child references that template id so the audit trail
--     reads "this ticket was generated from #N".
--
-- The rule itself stays on every ticket in the chain so closing a
-- generated child still spawns the next one even if the original
-- template gets archived.

ALTER TABLE tickets
    ADD COLUMN recurrence_rule TEXT,
    ADD COLUMN recurrence_template_id INTEGER REFERENCES tickets(id) ON DELETE SET NULL;

-- The hot path is "find every ticket that has a rule" (the close
-- handler reads this to decide whether to spawn). Partial index
-- keeps the work proportional to recurring tickets, not all of them.
CREATE INDEX tickets_recurrence_rule_idx
    ON tickets (id) WHERE recurrence_rule IS NOT NULL;
