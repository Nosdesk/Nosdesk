-- Revert the index predicate to the original NULL-only exemption. The blank ->
-- NULL backfill is not reversed: the original empty strings are unrecoverable
-- and were semantically "no serial" anyway.
DROP INDEX IF EXISTS idx_asset_serial_unique;
CREATE UNIQUE INDEX idx_asset_serial_unique
    ON public.assets USING btree (workspace_id, serial_number)
    WHERE (serial_number IS NOT NULL);
