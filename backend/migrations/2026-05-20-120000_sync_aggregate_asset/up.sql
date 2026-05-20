-- Asset as a sync aggregate. After Phase A renamed devices to
-- assets and Phase B/C wired in the runtime kind registry, the
-- inventory becomes a first-class entity that the frontend wants
-- to query reactively (kind filters, list views, ticket-attached
-- references) without per-row /api/devices round trips.
--
-- Mirrors the user-sync work (2026-05-09-200000): adding the
-- variant here unlocks emit::record(SyncEmit { aggregate:
-- SyncAggregate::Asset, ... }) for asset writes, and a
-- corresponding sync-models/asset.json descriptor wires the
-- frontend cache pool.
--
-- ALTER TYPE ... ADD VALUE is safe inside a transaction on
-- Postgres 12+; the new value just can't be referenced in the
-- same transaction that adds it.

ALTER TYPE sync_aggregate ADD VALUE IF NOT EXISTS 'asset';
