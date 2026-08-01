-- P2: activation-anchored SLA clock.
--
-- clock_start declares when a policy's clock starts:
--   'created'   — from ticket creation (client-facing "respond within N of
--                 submission" SLAs, which legitimately run during triage).
--   'activated' — from the ticket's first entry into a non-pausing (Active)
--                 state (internal targets that shouldn't burn while a ticket
--                 waits in backlog). Default: fixes the instant-breach bug.
ALTER TABLE sla_policies ADD COLUMN clock_start VARCHAR(16) NOT NULL DEFAULT 'activated';

-- The effective SLA start for a ticket (also pushed forward by paused business
-- time, so pausing subtracts). NULL = the clock has never started (an 'activated'
-- policy renders no SLA yet). Metadata-only add, no backfill: pre-migration
-- tickets self-heal on their next workflow transition (see services::sla).
ALTER TABLE tickets ADD COLUMN sla_clock_started_at TIMESTAMPTZ;
-- When the current pause began, so a resume can add back the paused business
-- time. NULL = not currently paused.
ALTER TABLE tickets ADD COLUMN sla_paused_at TIMESTAMPTZ;
