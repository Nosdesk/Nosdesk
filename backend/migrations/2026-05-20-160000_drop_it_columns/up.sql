-- Drop the 14 IT-flavoured columns from `assets` now that the
-- attributes JSONB blob holds the same data (per B1 backfill)
-- and every reader has switched to reading from it (per B2).
--
-- After this migration the `assets` table is genuinely generic:
-- universal core columns (id, name, kind, manufacturer, model,
-- serial_number, location, asset_tag, primary_user_uuid, notes,
-- purchase_date, quantity, unit, attributes, external_sync_source,
-- timestamps) and nothing IT-specific.
--
-- The matching Rust struct trim, the Intune sync writer rewrite,
-- and the DeviceView form cleanup ride in the same commit; once
-- this lands the backend will refuse to compile against the old
-- column names so the all-paths-touched property is enforced at
-- the type level.

ALTER TABLE assets DROP COLUMN hostname;
ALTER TABLE assets DROP COLUMN device_type;
ALTER TABLE assets DROP COLUMN operating_system;
ALTER TABLE assets DROP COLUMN os_version;
ALTER TABLE assets DROP COLUMN warranty_status;
ALTER TABLE assets DROP COLUMN warranty_start_date;
ALTER TABLE assets DROP COLUMN warranty_end_date;
ALTER TABLE assets DROP COLUMN compliance_state;
ALTER TABLE assets DROP COLUMN microsoft_device_id;
ALTER TABLE assets DROP COLUMN intune_device_id;
ALTER TABLE assets DROP COLUMN entra_device_id;
ALTER TABLE assets DROP COLUMN is_managed;
ALTER TABLE assets DROP COLUMN enrollment_date;
ALTER TABLE assets DROP COLUMN last_sync_time;
