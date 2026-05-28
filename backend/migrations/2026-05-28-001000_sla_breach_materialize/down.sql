DROP INDEX IF EXISTS tickets_sla_resolution_scan_idx;
DROP INDEX IF EXISTS tickets_sla_response_scan_idx;

ALTER TABLE tickets
    DROP COLUMN IF EXISTS sla_resolution_breached_at,
    DROP COLUMN IF EXISTS sla_resolution_target_at,
    DROP COLUMN IF EXISTS sla_response_breached_at,
    DROP COLUMN IF EXISTS sla_response_target_at;
