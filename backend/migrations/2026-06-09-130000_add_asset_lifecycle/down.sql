DROP TABLE IF EXISTS public.asset_lifecycle_events;
DROP SEQUENCE IF EXISTS public.asset_lifecycle_events_id_seq;

-- Restore the previous global serial uniqueness.
DROP INDEX IF EXISTS public.idx_asset_serial_unique;
CREATE UNIQUE INDEX idx_asset_serial_unique
    ON public.assets USING btree (serial_number)
    WHERE (serial_number IS NOT NULL);

-- Dropping the column also drops idx_assets_workspace_status.
ALTER TABLE public.assets DROP COLUMN IF EXISTS status;

-- Postgres cannot drop an individual enum label without rebuilding
-- the type and rewriting every dependent column, so the
-- `asset_lifecycle_event` sync_aggregate value is left in place.
