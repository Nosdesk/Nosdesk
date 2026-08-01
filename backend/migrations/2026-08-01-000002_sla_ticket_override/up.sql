-- P/C: a per-ticket SLA override, the manual escape hatch above policy
-- resolution. 'auto' = normal policy matching; 'none' = this ticket has no
-- SLA regardless of which policy would match (compute_pill short-circuits).
ALTER TABLE tickets ADD COLUMN sla_override VARCHAR(16) NOT NULL DEFAULT 'auto';
