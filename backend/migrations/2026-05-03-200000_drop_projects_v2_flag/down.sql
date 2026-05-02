-- Restore the workspace-level projects_v2 default to true. This
-- is the same write the 2026-05-03-100000 default-on migration
-- did; per-user overrides are not restored (the down direction
-- doesn't have the original values to put back).

UPDATE site_settings
   SET feature_flags = jsonb_set(
       COALESCE(feature_flags, '{}'::jsonb),
       '{projects_v2}',
       'true'::jsonb,
       true
   )
 WHERE id = 1;
