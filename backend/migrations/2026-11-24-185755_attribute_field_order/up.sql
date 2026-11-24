-- Asset kinds and the user field schema keep their fields' display order in
-- `field_order`, as record types do. jsonb doesn't keep the order keys were
-- written in, and the API reads a schema into a map sorted by key bytes, so
-- every client shows a schema's fields in that byte order today. Each row
-- starts from that order, so nothing moves until an admin reorders.
--
-- Both tables' audit triggers are off around the backfill: they need
-- app.workspace_id, which a migration session doesn't set (NDX01).

ALTER TABLE public.asset_kinds
    ADD COLUMN field_order text[] NOT NULL DEFAULT '{}';
ALTER TABLE public.user_field_schema
    ADD COLUMN field_order text[] NOT NULL DEFAULT '{}';

ALTER TABLE public.asset_kinds DISABLE TRIGGER tr_audit_asset_kinds;
ALTER TABLE public.user_field_schema DISABLE TRIGGER tr_audit_user_field_schema;

DO $backfill$
DECLARE
    kinds_ordered integer;
    user_schemas_ordered integer;
BEGIN
    UPDATE public.asset_kinds
    SET field_order = ARRAY(
        SELECT k FROM jsonb_object_keys(attribute_schema -> 'properties') AS k
        ORDER BY k COLLATE "C"
    )
    WHERE jsonb_typeof(attribute_schema -> 'properties') = 'object';
    GET DIAGNOSTICS kinds_ordered = ROW_COUNT;

    UPDATE public.user_field_schema
    SET field_order = ARRAY(
        SELECT k FROM jsonb_object_keys(schema -> 'properties') AS k
        ORDER BY k COLLATE "C"
    )
    WHERE jsonb_typeof(schema -> 'properties') = 'object';
    GET DIAGNOSTICS user_schemas_ordered = ROW_COUNT;

    RAISE NOTICE 'field order: % asset kinds, % user field schemas', kinds_ordered, user_schemas_ordered;
END
$backfill$;

ALTER TABLE public.user_field_schema ENABLE TRIGGER tr_audit_user_field_schema;
ALTER TABLE public.asset_kinds ENABLE TRIGGER tr_audit_asset_kinds;
