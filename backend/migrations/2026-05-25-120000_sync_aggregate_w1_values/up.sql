-- C/W1: add the tier-1 aggregate values the audit-log plan enumerates
-- but that were never added to the sync_aggregate Postgres enum.
--
-- emit::record casts the aggregate string to ::sync_aggregate, so the
-- DB enum must carry every value before any repository can emit it.
-- These five clear the `sync-pending-wire` markers on webhooks,
-- channels, knowledge_gaps, documentation, and documentation_collections
-- (the Rust SyncAggregate variants + sync-models manifests + the emit
-- calls themselves land per-aggregate alongside this).
--
-- ALTER TYPE ... ADD VALUE is permitted inside a transaction on
-- PG 12+ as long as the new value isn't *used* in the same
-- transaction; this migration only adds them, so it's safe under
-- Diesel's per-migration transaction. IF NOT EXISTS keeps it
-- idempotent across re-runs.

ALTER TYPE sync_aggregate ADD VALUE IF NOT EXISTS 'webhook';
ALTER TYPE sync_aggregate ADD VALUE IF NOT EXISTS 'channel';
ALTER TYPE sync_aggregate ADD VALUE IF NOT EXISTS 'knowledge_gap';
ALTER TYPE sync_aggregate ADD VALUE IF NOT EXISTS 'documentation_page';
ALTER TYPE sync_aggregate ADD VALUE IF NOT EXISTS 'documentation_collection';
