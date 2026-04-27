-- Indexes supporting the consolidated /api/dashboard/stats endpoint.
--
-- The stats endpoint runs three grouped scans over `tickets`
-- (queue / assignee-scoped / requester-scoped) plus a few count
-- queries on closed_at + unassigned. Without these indexes each
-- call is a sequential scan of the entire tickets table; with
-- them it's a small index seek + bucketed count.
--
-- Partial indexes are used where the column is sparse (the
-- assignee / requester filters and the `closed` status filter)
-- to keep the indexes small and fast.

CREATE INDEX IF NOT EXISTS idx_tickets_status_priority
    ON tickets(status, priority);

CREATE INDEX IF NOT EXISTS idx_tickets_assignee_status_priority
    ON tickets(assignee_uuid, status, priority)
    WHERE assignee_uuid IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_tickets_requester_status_priority
    ON tickets(requester_uuid, status, priority)
    WHERE requester_uuid IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_tickets_closed_at
    ON tickets(closed_at)
    WHERE status = 'closed';

CREATE INDEX IF NOT EXISTS idx_tickets_unassigned_status
    ON tickets(status)
    WHERE assignee_uuid IS NULL;
