ALTER TABLE devices
    ADD COLUMN warranty_start_date DATE,
    ADD COLUMN warranty_end_date DATE,
    ADD COLUMN purchase_date DATE,
    ADD COLUMN asset_tag VARCHAR(255);

CREATE INDEX idx_devices_warranty_end_date ON devices (warranty_end_date)
    WHERE warranty_end_date IS NOT NULL;

CREATE INDEX idx_devices_asset_tag ON devices (asset_tag)
    WHERE asset_tag IS NOT NULL;
