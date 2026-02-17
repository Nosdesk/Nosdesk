DROP INDEX IF EXISTS idx_devices_asset_tag;
DROP INDEX IF EXISTS idx_devices_warranty_end_date;
ALTER TABLE devices
    DROP COLUMN IF EXISTS asset_tag,
    DROP COLUMN IF EXISTS purchase_date,
    DROP COLUMN IF EXISTS warranty_end_date,
    DROP COLUMN IF EXISTS warranty_start_date;
