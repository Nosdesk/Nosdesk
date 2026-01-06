-- Revert: Rename microsoft_device_id back to azure_device_id
ALTER TABLE devices RENAME COLUMN microsoft_device_id TO azure_device_id;
