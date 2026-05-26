-- Item C / W5b: add the `data` value to the sync_aggregate enum so
-- emit::record can write the synthetic data.audit.read /
-- data.audit.exported meta events (the SyncAggregate::Data variant +
-- sync-models/data.json manifest land alongside this).
--
-- emit::record casts the aggregate string to ::sync_aggregate, so the
-- DB enum must carry the value before the audit handler can emit it.

ALTER TYPE sync_aggregate ADD VALUE IF NOT EXISTS 'data';
