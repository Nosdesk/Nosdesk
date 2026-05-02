-- Reversible: substrate is invisible to product code at this stage.
-- Once Phase 2 wires the emit::record helper into repositories the
-- down migration becomes lossy (events captured during the window
-- between up and down are dropped); rolling back later requires
-- exporting sync_actions to a snapshot first.

DROP TABLE IF EXISTS system_meta;

DROP FUNCTION IF EXISTS audit_log_trigger();
DROP TABLE IF EXISTS audit_log_2026_08;
DROP TABLE IF EXISTS audit_log_2026_07;
DROP TABLE IF EXISTS audit_log_2026_06;
DROP TABLE IF EXISTS audit_log_2026_05;
DROP TABLE IF EXISTS audit_log;

DROP TABLE IF EXISTS sync_actions_2026_08;
DROP TABLE IF EXISTS sync_actions_2026_07;
DROP TABLE IF EXISTS sync_actions_2026_06;
DROP TABLE IF EXISTS sync_actions_2026_05;
DROP TABLE IF EXISTS sync_actions;

DROP TYPE IF EXISTS sync_aggregate;
DROP TYPE IF EXISTS sync_op;
