-- Reverse the B1 backfill. Note that this DOES NOT restore the
-- IT columns from attribute backfill if those columns have been
-- subsequently dropped by B3; this down.sql is meaningful only
-- before B3 lands. If you need to revert B3, restore the columns
-- in that migration's down.sql first.
--
-- The attributes JSONB rewrite is not perfectly reversible -- we
-- can strip the 13 IT keys we know we put in, but if anything
-- legitimately stored a key with one of those names mid-flight
-- between B1 and the down, this would clobber it. Practical
-- reverts run immediately after a bad up.sql so the window is
-- microseconds.

UPDATE assets
SET attributes = attributes
    - 'hostname'
    - 'operating_system'
    - 'os_version'
    - 'warranty_status'
    - 'warranty_start_date'
    - 'warranty_end_date'
    - 'compliance_state'
    - 'microsoft_device_id'
    - 'intune_device_id'
    - 'entra_device_id'
    - 'is_managed'
    - 'enrollment_date'
    - 'last_sync_time';

UPDATE asset_kinds
SET attribute_schema = '{"type":"object","properties":{}}'::jsonb
WHERE category = 'it';

ALTER TABLE assets DROP COLUMN external_sync_source;
