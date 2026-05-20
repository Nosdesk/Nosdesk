-- Restore the dropped IT columns. Backfills from the attributes
-- JSONB blob so existing rows recover their hostname / warranty
-- / etc. data (B1 left it there). Rows that had never had a
-- value for a given key (e.g. a non-IT material) end up with
-- NULL, matching pre-B3 semantics.

ALTER TABLE assets ADD COLUMN hostname             VARCHAR(255);
ALTER TABLE assets ADD COLUMN device_type          VARCHAR(100);
ALTER TABLE assets ADD COLUMN operating_system     VARCHAR(100);
ALTER TABLE assets ADD COLUMN os_version           VARCHAR(100);
ALTER TABLE assets ADD COLUMN warranty_status      VARCHAR(50);
ALTER TABLE assets ADD COLUMN warranty_start_date  DATE;
ALTER TABLE assets ADD COLUMN warranty_end_date    DATE;
ALTER TABLE assets ADD COLUMN compliance_state     VARCHAR(50);
ALTER TABLE assets ADD COLUMN microsoft_device_id  VARCHAR(255);
ALTER TABLE assets ADD COLUMN intune_device_id     VARCHAR(255);
ALTER TABLE assets ADD COLUMN entra_device_id      VARCHAR(255);
ALTER TABLE assets ADD COLUMN is_managed           BOOLEAN;
ALTER TABLE assets ADD COLUMN enrollment_date      TIMESTAMPTZ;
ALTER TABLE assets ADD COLUMN last_sync_time       TIMESTAMPTZ;

UPDATE assets SET
    hostname             = attributes->>'hostname',
    operating_system     = attributes->>'operating_system',
    os_version           = attributes->>'os_version',
    warranty_status      = attributes->>'warranty_status',
    warranty_start_date  = NULLIF(attributes->>'warranty_start_date', '')::date,
    warranty_end_date    = NULLIF(attributes->>'warranty_end_date', '')::date,
    compliance_state     = attributes->>'compliance_state',
    microsoft_device_id  = attributes->>'microsoft_device_id',
    intune_device_id     = attributes->>'intune_device_id',
    entra_device_id      = attributes->>'entra_device_id',
    is_managed           = (attributes->>'is_managed')::boolean,
    enrollment_date      = NULLIF(attributes->>'enrollment_date', '')::timestamptz,
    last_sync_time       = NULLIF(attributes->>'last_sync_time', '')::timestamptz;
