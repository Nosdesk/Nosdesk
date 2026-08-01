ALTER TABLE tickets DROP COLUMN sla_paused_at;
ALTER TABLE tickets DROP COLUMN sla_clock_started_at;
ALTER TABLE sla_policies DROP COLUMN clock_start;
