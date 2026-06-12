-- Workspace-leading composite indexes for the dashboard analytics
-- aggregations (KPI summary, timeseries, breakdowns).
--
-- The initial schema's idx_tickets_created_at / idx_tickets_closed_at
-- lead with the time column. Under pooled multi-tenancy every analytics
-- query is RLS-scoped (workspace_id = current_setting('app.workspace_id')),
-- so a time-leading index makes the scan walk the global time range across
-- every tenant before filtering down to one workspace. Leading with
-- workspace_id keeps each tenant's scan local to its own rows.

-- created_at-windowed aggregates: created count, created timeseries,
-- and the breakdowns (all filter created_at within the workspace).
CREATE INDEX IF NOT EXISTS idx_tickets_ws_created_at
    ON tickets (workspace_id, created_at);

-- closed_at-windowed aggregates: resolved count and resolved timeseries.
-- Partial on the non-null side, mirroring idx_tickets_closed_at, since
-- only closed tickets carry a closed_at.
CREATE INDEX IF NOT EXISTS idx_tickets_ws_closed_at
    ON tickets (workspace_id, closed_at)
    WHERE closed_at IS NOT NULL;

-- Open snapshot (closed_at IS NULL). Today this has no supporting index
-- and resolves to a sequential scan; the existing idx_tickets_closed_at
-- covers the opposite (non-null) side. Partial + workspace-leading so the
-- "tickets currently open" KPI is a per-tenant index scan.
CREATE INDEX IF NOT EXISTS idx_tickets_ws_open
    ON tickets (workspace_id)
    WHERE closed_at IS NULL;
