-- Covering composites for the dashboard breakdown + leaderboard
-- aggregations. Extends the workspace-leading principle from the
-- previous migration: leading (workspace_id, created_at) keeps the
-- windowed scan tenant-local, and the trailing group column lets the
-- count-by-X run as an index-only scan, with no per-row heap fetch to
-- read the grouped column.
--
-- Tradeoff: four extra btrees on the hot tickets table. The grouped
-- columns change infrequently (priority/category at triage, assignee on
-- reassignment, requester effectively never), so write amplification is
-- modest relative to the read win on staff dashboards.

-- breakdown by priority
CREATE INDEX IF NOT EXISTS idx_tickets_ws_created_priority
    ON tickets (workspace_id, created_at, priority);

-- breakdown by category (non-partial: the breakdown counts the NULL
-- "uncategorised" bucket, so the index must include NULL category rows)
CREATE INDEX IF NOT EXISTS idx_tickets_ws_created_category
    ON tickets (workspace_id, created_at, category_id);

-- breakdown by assignee AND leaderboard by assignee. Non-partial: the
-- breakdown counts the NULL "unassigned" bucket; the leaderboard filters
-- assignee_uuid IS NOT NULL but can still satisfy that from this index.
CREATE INDEX IF NOT EXISTS idx_tickets_ws_created_assignee
    ON tickets (workspace_id, created_at, assignee_uuid);

-- leaderboard by requester. Partial on the non-null side: the only
-- consumer (the leaderboard) filters requester_uuid IS NOT NULL, and
-- there is no requester breakdown that would need the NULL rows.
CREATE INDEX IF NOT EXISTS idx_tickets_ws_created_requester
    ON tickets (workspace_id, created_at, requester_uuid)
    WHERE requester_uuid IS NOT NULL;
