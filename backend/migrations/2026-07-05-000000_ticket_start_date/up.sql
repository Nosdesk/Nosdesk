-- Optional planning start for timeline (gantt) scheduling. NULL means
-- unplanned; the gantt falls back to created_at for a bar's left edge.
-- Nullable with no backfill, so no audit-trigger handling is needed.
ALTER TABLE tickets ADD COLUMN start_date TIMESTAMPTZ;
