-- Extend the asset usage ledger with a direction discriminator
-- so the same table can carry both decrement (usage) and
-- increment (restock) events. The plumbing-business use case:
-- a technician orders 100 metres of pipe, the dispatcher records
-- "received 100 m" as a restock event, the on-hand quantity
-- increments and the row sits in the same audit trail as
-- subsequent usage decrements.
--
-- Both event kinds keep `quantity_used > 0`; the column carries
-- magnitude, the `event_kind` carries direction. We don't allow
-- negative magnitudes because the CHECK constraint is the only
-- thing keeping a bad row from claiming to "use" -5 m of pipe;
-- splitting on event_kind is the safer model. The column name
-- stays `quantity_used` for compatibility with v1 SELECTs but
-- semantically it's "magnitude" once you read event_kind.
--
-- Existing rows are usage events (the only kind shipped in v1).

ALTER TABLE asset_usage_log
  ADD COLUMN event_kind VARCHAR(16) NOT NULL DEFAULT 'usage'
    CHECK (event_kind IN ('usage', 'restock'));

-- Drop the default once the column is populated. Future inserts
-- must specify event_kind explicitly so callers can't fall back
-- to 'usage' by accident on a restock path.
ALTER TABLE asset_usage_log
  ALTER COLUMN event_kind DROP DEFAULT;
