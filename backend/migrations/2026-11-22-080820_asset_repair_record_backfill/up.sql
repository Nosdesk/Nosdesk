-- Repairs entered before records (#736) become repair records, so reports
-- have one source. Until now a move into in_repair kept its details in the
-- event's metadata (vendor, rma_number, offsite, expected_return); each such
-- event becomes a repair record linked to it:
--
--   occurred_at   the event's occurred_at, its day in the workspace timezone;
--   completed_at  the asset's next lifecycle event, the move out of repair;
--                 none for an asset still in repair (an open record);
--   vendor        metadata.vendor; reference metadata.rma_number;
--   attributes    offsite and expected_return;
--   ticket_id     the event's ticket; lifecycle_event_id the event;
--   recorded_*    the event's occurred_at and actor, who entered the facts;
--   source        backfill. No outcome: nobody recorded one.
--
-- Not carried: moves made by an import (metadata.import_job_id) or a rule
-- (metadata.rule_id), bare moves with no usable detail, and events that
-- already have a record. Metadata is read defensively: a value that isn't a
-- string (or a number, for the two texts), is blank, or isn't a real
-- YYYY-MM-DD date is left out, and texts are cut to their columns. Event
-- metadata is left as it was.
--
-- Record types are a catalogue a workspace adds from, so the repair type is
-- added only to a workspace with repairs to carry and without it already,
-- with the values record_catalogue::CATALOGUE's repair entry gives
-- (add_from_catalogue): built in, no name of its own, opening with
-- in_repair unless another live type already does.
--
-- Both tables' audit triggers are off around the inserts: they need
-- app.workspace_id, which a migration session doesn't set (NDX01).

ALTER TABLE public.asset_record_types DISABLE TRIGGER tr_audit_asset_record_types;
ALTER TABLE public.asset_records DISABLE TRIGGER tr_audit_asset_records;

DO $backfill$
DECLARE
    types_added integer;
    records_added integer;
    still_open integer;
    open_not_in_repair integer;
    skipped_import integer;
    skipped_rule integer;
    skipped_bare integer;
BEGIN
    CREATE TEMPORARY TABLE repair_backfill ON COMMIT DROP AS
    WITH events AS (
        SELECT e.id, e.workspace_id, e.asset_id, e.occurred_at, e.ticket_id, e.actor_uuid,
               CASE WHEN jsonb_typeof(e.metadata) = 'object' THEN e.metadata
                    ELSE '{}'::jsonb END AS m
        FROM public.asset_lifecycle_events e
        WHERE e.to_status = 'in_repair'
          AND NOT EXISTS (SELECT 1 FROM public.asset_records r
                          WHERE r.workspace_id = e.workspace_id
                            AND r.lifecycle_event_id = e.id)
    ),
    cleaned AS (
        SELECT ev.*,
               left(nullif(btrim(CASE WHEN jsonb_typeof(m -> 'vendor') IN ('string', 'number')
                                      THEN m ->> 'vendor' END), ''), 255) AS vendor,
               left(nullif(btrim(CASE WHEN jsonb_typeof(m -> 'rma_number') IN ('string', 'number')
                                      THEN m ->> 'rma_number' END), ''), 255) AS reference,
               CASE jsonb_typeof(m -> 'offsite')
                   WHEN 'boolean' THEN m -> 'offsite'
                   WHEN 'string' THEN CASE lower(btrim(m ->> 'offsite'))
                                          WHEN 'true' THEN 'true'::jsonb
                                          WHEN 'false' THEN 'false'::jsonb END
               END AS offsite,
               CASE WHEN jsonb_typeof(m -> 'expected_return') = 'string'
                         AND btrim(m ->> 'expected_return') ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}($|[T ])'
                         AND pg_input_is_valid(left(btrim(m ->> 'expected_return'), 10), 'date')
                    THEN left(btrim(m ->> 'expected_return'), 10) END AS expected_return,
               m ? 'import_job_id' AS by_import,
               m ? 'rule_id' AS by_rule,
               m ? 'record_id' AS has_record
        FROM events ev
    )
    SELECT c.*,
           NOT c.by_import AND NOT c.by_rule AND NOT c.has_record
           AND (c.vendor IS NOT NULL OR c.reference IS NOT NULL
                OR c.offsite IS NOT DISTINCT FROM 'true'::jsonb
                OR c.expected_return IS NOT NULL) AS carried
    FROM cleaned c;

    SELECT count(*) FILTER (WHERE by_import),
           count(*) FILTER (WHERE by_rule AND NOT by_import),
           count(*) FILTER (WHERE NOT carried AND NOT by_import AND NOT by_rule AND NOT has_record)
    INTO skipped_import, skipped_rule, skipped_bare
    FROM repair_backfill;

    -- The repair type, for workspaces with repairs to carry and without it.
    INSERT INTO public.asset_record_types
        (workspace_id, slug, label, description, icon, color, field_schema, field_order,
         core_fields, outcomes, applies_to_kinds, opens_with_status, is_builtin, sort_order,
         created_by)
    SELECT w.workspace_id, 'repair', NULL, NULL, 'settings', NULL,
           '{"type": "object", "properties": {
               "fault": {"type": "string"},
               "offsite": {"type": "boolean"},
               "expected_return": {"type": "string", "format": "date"},
               "serial_received": {"type": "string"}}}'::jsonb,
           ARRAY['fault', 'offsite', 'expected_return', 'serial_received'],
           '{}'::jsonb,
           '[{"key": "repaired", "category": "done"},
             {"key": "replaced", "category": "replaced"},
             {"key": "beyond_repair", "category": "not_done"},
             {"key": "no_fault_found", "category": "no_fault"}]'::jsonb,
           NULL,
           CASE WHEN EXISTS (SELECT 1 FROM public.asset_record_types o
                             WHERE o.workspace_id = w.workspace_id
                               AND o.opens_with_status = 'in_repair'
                               AND o.archived_at IS NULL)
                THEN NULL ELSE 'in_repair' END,
           true, 10, NULL
    FROM (SELECT DISTINCT workspace_id FROM repair_backfill WHERE carried) w
    WHERE NOT EXISTS (SELECT 1 FROM public.asset_record_types t
                      WHERE t.workspace_id = w.workspace_id AND t.slug = 'repair')
    ON CONFLICT (workspace_id, slug) DO NOTHING;
    GET DIAGNOSTICS types_added = ROW_COUNT;

    INSERT INTO public.asset_records
        (workspace_id, asset_id, record_type_id, occurred_on, occurred_at, completed_on,
         completed_at, vendor, reference, ticket_id, snapshot_source, attributes,
         lifecycle_event_id, source, recorded_at, recorded_by, updated_at, updated_by)
    SELECT b.workspace_id, b.asset_id, t.id,
           (b.occurred_at AT TIME ZONE z.tz)::date, b.occurred_at,
           (done.at AT TIME ZONE z.tz)::date, done.at,
           b.vendor, b.reference, b.ticket_id, 'none',
           -- Only values the workspace's repair type has a field for, of
           -- that field's type.
           (SELECT coalesce(jsonb_object_agg(a.key, a.value), '{}'::jsonb)
            FROM jsonb_each(jsonb_strip_nulls(jsonb_build_object(
                     'offsite', b.offsite, 'expected_return', to_jsonb(b.expected_return)))) a
            WHERE t.field_schema -> 'properties' -> a.key ->> 'type' = jsonb_typeof(a.value)
              AND coalesce(t.field_schema -> 'properties' -> a.key ->> 'format', 'date') = 'date'),
           b.id, 'backfill', b.occurred_at, b.actor_uuid, b.occurred_at, b.actor_uuid
    FROM repair_backfill b
    JOIN public.asset_record_types t
      ON t.workspace_id = b.workspace_id AND t.slug = 'repair'
    -- The workspace's timezone, UTC when it isn't one Postgres knows.
    CROSS JOIN LATERAL (
        SELECT coalesce((SELECT s.default_timezone FROM public.site_settings s
                         WHERE s.workspace_id = b.workspace_id
                           AND s.default_timezone IN (SELECT name FROM pg_timezone_names)
                         LIMIT 1), 'UTC') AS tz
    ) z
    -- The move out of repair: the asset's next event.
    LEFT JOIN LATERAL (
        SELECT greatest(n.occurred_at, b.occurred_at) AS at
        FROM public.asset_lifecycle_events n
        WHERE n.workspace_id = b.workspace_id
          AND n.asset_id = b.asset_id
          AND (n.occurred_at, n.id) > (b.occurred_at, b.id)
        ORDER BY n.occurred_at, n.id
        LIMIT 1
    ) done ON true
    WHERE b.carried
      AND (b.occurred_at AT TIME ZONE z.tz)::date >= DATE '1900-01-01';
    GET DIAGNOSTICS records_added = ROW_COUNT;

    SELECT count(*) FILTER (WHERE r.completed_on IS NULL),
           count(*) FILTER (WHERE r.completed_on IS NULL AND a.status <> 'in_repair')
    INTO still_open, open_not_in_repair
    FROM public.asset_records r
    JOIN public.assets a ON a.workspace_id = r.workspace_id AND a.id = r.asset_id
    WHERE r.source = 'backfill';

    RAISE NOTICE 'repair backfill: % records (% open, % of them on assets no longer in repair), '
                 '% repair types added; skipped % import moves, % rule moves, % without details',
        records_added, still_open, open_not_in_repair, types_added,
        skipped_import, skipped_rule, skipped_bare;
END
$backfill$;

ALTER TABLE public.asset_records ENABLE TRIGGER tr_audit_asset_records;
ALTER TABLE public.asset_record_types ENABLE TRIGGER tr_audit_asset_record_types;
