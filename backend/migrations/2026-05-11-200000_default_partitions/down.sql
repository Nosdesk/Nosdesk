-- Reverses 2026-05-11-200000_default_partitions/up.sql.
--
-- Detach before drop so the parent's lock window is bounded — same dual
-- of W6a's lock-friendly attach pattern. DETACH PARTITION CONCURRENTLY
-- is unavailable inside a migration's implicit transaction (it requires
-- its own transaction); the plain DETACH form is acceptable here since
-- this migration is intended for dev-environment reset and not online
-- production rollback.
ALTER TABLE audit_log DETACH PARTITION audit_log_default;
DROP TABLE audit_log_default;

ALTER TABLE sync_actions DETACH PARTITION sync_actions_default;
DROP TABLE sync_actions_default;
