-- Cycle as a sync aggregate. Cycle CRUD and ticket-cycle moves
-- become first-class typed events on the sync_actions stream so
-- the burndown widget can rebuild from domain_events filtered to
-- a `cycle:N` group.
--
-- ALTER TYPE ... ADD VALUE inside a transaction is fine on
-- Postgres 12+; the new value just can't be used in the same
-- transaction. compose.yaml pins postgres:18.

ALTER TYPE sync_aggregate ADD VALUE IF NOT EXISTS 'cycle';
ALTER TYPE sync_aggregate ADD VALUE IF NOT EXISTS 'cycle_ticket';
