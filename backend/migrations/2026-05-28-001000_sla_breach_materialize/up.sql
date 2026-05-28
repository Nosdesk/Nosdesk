-- SLA Phase 1c — materialised breach-scan columns.
--
-- The SLA pill itself stays compute-on-read (the JSON shape the
-- frontend reads is derived live by services::sla::compute_pill), but
-- the breach-detection job (Phase 2) needs a cheap indexed scan over
-- millions of tickets. These columns mirror the response + resolution
-- timers' computed target times so the job can SELECT
--     WHERE sla_*_target_at <= NOW() AND sla_*_breached_at IS NULL
-- without rewalking the policy / calendar / holiday context per row.
--
-- Two pairs (response + resolution) because each timer breaches
-- independently — one ticket can have an on-time response but a
-- breached resolution, and we want a notification for each.
--
-- Target columns are stamped to NULL when the timer either has no
-- policy target configured, has been met (response only — set when
-- first_response_at lands), or is paused (the ticket isn't in an
-- active workflow state). Partial indexes then exclude the NULL rows
-- from the scan plan so a workspace with mostly closed/paused tickets
-- pays for almost nothing.
--
-- breached_at columns dual-purpose as idempotency stamps: the job
-- only fires a notification/webhook for rows where the column is NULL
-- and the target has passed. Once stamped, the row drops out of the
-- scan via the partial index predicate.
--
-- Note: no backfill in this migration. Stamping happens in the
-- mutation paths (update_ticket_partial + comment first-response
-- stamp) going forward; the Phase 2 job will do a stamp-unstamped
-- pass on each tick so tickets that haven't mutated since this
-- migration land in the breach-scan eligible set on the next loop.

ALTER TABLE tickets
    ADD COLUMN sla_response_target_at TIMESTAMPTZ,
    ADD COLUMN sla_response_breached_at TIMESTAMPTZ,
    ADD COLUMN sla_resolution_target_at TIMESTAMPTZ,
    ADD COLUMN sla_resolution_breached_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS tickets_sla_response_scan_idx
    ON tickets (sla_response_target_at)
    WHERE sla_response_target_at IS NOT NULL
      AND sla_response_breached_at IS NULL;

CREATE INDEX IF NOT EXISTS tickets_sla_resolution_scan_idx
    ON tickets (sla_resolution_target_at)
    WHERE sla_resolution_target_at IS NOT NULL
      AND sla_resolution_breached_at IS NULL;
