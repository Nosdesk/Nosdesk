-- Pass B1: backfill IT-flavoured columns into the per-row
-- attributes JSONB blob so a future commit can drop the columns
-- from `assets` without losing data.
--
-- Three changes, no column drops:
--
-- 1. Add `external_sync_source` to `assets`. Replaces the
--    "intune_device_id IS NOT NULL OR entra_device_id IS NOT NULL"
--    predicate that powered `is_editable`. Top-level column (not
--    an attribute) because the read-only-from-external-sync
--    state isn't kind-specific.
--
-- 2. Seed the IT baseline attribute_schema onto every IT-category
--    `asset_kinds` row. The IT builtins currently ship with an
--    empty `{"type":"object","properties":{}}` schema, so the
--    overwrite is safe; if an admin had customised one of these
--    kinds since seed-time, their schema would be lost. Migration
--    log warns about this and the down.sql restores empty
--    schemas for IT kinds.
--
-- 3. Copy every IT column value on every asset row into the
--    `attributes` JSONB blob. `jsonb_strip_nulls` keeps the
--    output lean (a row with no `hostname` doesn't carry a
--    `hostname: null` key). Existing attribute keys win over the
--    backfilled values, so any kind-specific data already in
--    `attributes` is preserved.
--
-- B3 follows up by dropping the IT columns from `assets` once
-- B2 has switched every code path to read from `attributes`.

-- 1. external_sync_source --------------------------------------

ALTER TABLE assets ADD COLUMN external_sync_source VARCHAR(32);

UPDATE assets SET external_sync_source = CASE
    WHEN intune_device_id IS NOT NULL AND intune_device_id <> '' THEN 'intune'
    WHEN entra_device_id  IS NOT NULL AND entra_device_id  <> '' THEN 'entra'
    ELSE NULL
END;

-- 2. IT baseline attribute_schema ------------------------------

UPDATE asset_kinds
SET attribute_schema = '{
  "type": "object",
  "properties": {
    "hostname":             { "type": "string",  "title": "Hostname" },
    "operating_system":     { "type": "string",  "title": "Operating system" },
    "os_version":           { "type": "string",  "title": "OS version" },
    "warranty_status":      { "type": "string",  "title": "Warranty status",
                              "enum": ["Active", "Warning", "Expired", "Unknown"] },
    "warranty_start_date":  { "type": "string",  "title": "Warranty start", "format": "date" },
    "warranty_end_date":    { "type": "string",  "title": "Warranty end",   "format": "date" },
    "compliance_state":     { "type": "string",  "title": "Compliance state" },
    "microsoft_device_id":  { "type": "string",  "title": "Microsoft device ID" },
    "intune_device_id":     { "type": "string",  "title": "Intune device ID" },
    "entra_device_id":      { "type": "string",  "title": "Entra device ID" },
    "is_managed":           { "type": "boolean", "title": "Managed" },
    "enrollment_date":      { "type": "string",  "title": "Enrollment date", "format": "date-time" },
    "last_sync_time":       { "type": "string",  "title": "Last sync time",  "format": "date-time" }
  }
}'::jsonb
WHERE category = 'it';

-- 3. Per-row attribute backfill --------------------------------

-- jsonb_strip_nulls trims keys whose value is NULL so we don't
-- pad every row with hostname: null when the column was unset.
-- Right-hand operand of || wins; we put the existing attributes
-- on the right so kind-specific data already there beats the
-- backfilled column copy.
UPDATE assets
SET attributes = jsonb_strip_nulls(jsonb_build_object(
    'hostname',             hostname,
    'operating_system',     operating_system,
    'os_version',           os_version,
    'warranty_status',      warranty_status,
    'warranty_start_date',  warranty_start_date::text,
    'warranty_end_date',    warranty_end_date::text,
    'compliance_state',     compliance_state,
    'microsoft_device_id',  microsoft_device_id,
    'intune_device_id',     intune_device_id,
    'entra_device_id',      entra_device_id,
    'is_managed',           is_managed,
    'enrollment_date',      enrollment_date,
    'last_sync_time',       last_sync_time
)) || COALESCE(attributes, '{}'::jsonb);
