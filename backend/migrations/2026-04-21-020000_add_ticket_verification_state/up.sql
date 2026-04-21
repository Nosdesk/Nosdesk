-- Verification state for guest-submitted tickets.
-- NULL: not a gated submission (authenticated tickets, or guest tickets with
--       email verification disabled at submission time).
-- 'pending': guest submission awaiting the submitter's email confirmation.
--            Hidden from tech queue, search, and SSE until verified.
-- 'verified': submission has been confirmed via the verification link.
ALTER TABLE tickets
    ADD COLUMN verification_state VARCHAR(32);

-- Partial index so "show me all pending tickets" stays fast without adding
-- an index over the common NULL path.
CREATE INDEX idx_tickets_verification_state_pending
    ON tickets(verification_state)
    WHERE verification_state = 'pending';
