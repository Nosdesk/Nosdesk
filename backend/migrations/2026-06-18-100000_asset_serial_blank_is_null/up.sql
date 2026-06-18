-- Issue #24: a blank serial number must mean "no serial" (NULL), never ''.
--
-- idx_asset_serial_unique is a PARTIAL unique index that exempts only NULL
-- (Postgres treats each NULL as distinct). When "no serial" was stored as the
-- empty string instead of NULL, a second serial-less asset in the same
-- workspace collided on that index. The application now normalises blanks to
-- NULL at the repository boundary; this migration cleans existing rows and
-- widens the index predicate so a stray '' can never collide again.

-- Backfill: collapse blank / whitespace-only serials to NULL. `assets` is
-- audited (tr_audit_assets); its trigger inserts into audit_log using
-- app.workspace_id, which is unset during a migration, so a bulk UPDATE would
-- crash with a NOT NULL violation (NDX01). Disable the audit trigger for the
-- backfill (migrations aren't app-level changes that need auditing), then
-- re-enable it.
ALTER TABLE public.assets DISABLE TRIGGER tr_audit_assets;
UPDATE public.assets SET serial_number = NULL WHERE btrim(serial_number) = '';
ALTER TABLE public.assets ENABLE TRIGGER tr_audit_assets;

-- Harden the uniqueness predicate: only a non-blank serial is unique per
-- workspace. NULL and '' are both "no serial" and exempt. btrim() is IMMUTABLE,
-- so it is valid in a partial-index predicate.
DROP INDEX IF EXISTS idx_asset_serial_unique;
CREATE UNIQUE INDEX idx_asset_serial_unique
    ON public.assets USING btree (workspace_id, serial_number)
    WHERE (serial_number IS NOT NULL AND btrim(serial_number) <> '');
