-- Down migration: invert the rename + drop the new columns and
-- registry table. Data in `assets.attributes` / `quantity` /
-- `unit` is dropped; the discriminator goes with it. Rows
-- created under non-device kinds become indistinguishable from
-- devices after the down migration runs, so the down path is
-- safe only when no non-device rows exist.

DROP TABLE IF EXISTS asset_kinds;

DROP INDEX IF EXISTS idx_assets_kind;

ALTER TABLE assets DROP COLUMN IF EXISTS unit;
ALTER TABLE assets DROP COLUMN IF EXISTS quantity;
ALTER TABLE assets DROP COLUMN IF EXISTS attributes;
ALTER TABLE assets DROP COLUMN IF EXISTS kind;

ALTER INDEX idx_asset_groups_external RENAME TO idx_device_groups_external;
ALTER INDEX idx_asset_groups_group RENAME TO idx_device_groups_group;
ALTER INDEX idx_asset_groups_asset RENAME TO idx_device_groups_device;
ALTER INDEX idx_assets_asset_tag RENAME TO idx_devices_asset_tag;
ALTER INDEX idx_assets_warranty_end_date RENAME TO idx_devices_warranty_end_date;
ALTER INDEX idx_assets_created_at RENAME TO idx_devices_created_at;
ALTER INDEX idx_assets_serial_number RENAME TO idx_devices_serial_number;
ALTER INDEX idx_assets_primary_user RENAME TO idx_devices_primary_user;
ALTER INDEX idx_asset_serial_unique RENAME TO idx_device_serial_unique;

ALTER TABLE ticket_assets RENAME COLUMN asset_id TO device_id;
ALTER TABLE asset_groups RENAME COLUMN asset_id TO device_id;

ALTER TABLE ticket_assets RENAME TO ticket_devices;
ALTER TABLE asset_groups RENAME TO device_groups;
ALTER TABLE assets RENAME TO devices;
