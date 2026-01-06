-- Rename azure_device_id to microsoft_device_id for clarity
-- This column stores the Microsoft device identifier (deviceId field from Graph API)
ALTER TABLE devices RENAME COLUMN azure_device_id TO microsoft_device_id;
