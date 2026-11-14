-- Postgres does not drop enum values; 'paused' stays, unused once the next
-- migration's down moves paused rules back to dry_run.
SELECT 1;
