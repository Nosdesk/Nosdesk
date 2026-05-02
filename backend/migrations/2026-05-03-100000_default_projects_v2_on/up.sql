-- Phase 5 milestone: flip projects_v2 to default-on at the
-- workspace level. Existing per-user overrides (set via the admin
-- UI) win; this only changes the default for users who haven't
-- expressed a preference. Reversible by re-running the same
-- statement with `false`.

UPDATE site_settings
   SET feature_flags = jsonb_set(
       COALESCE(feature_flags, '{}'::jsonb),
       '{projects_v2}',
       'true'::jsonb,
       true
   )
 WHERE id = 1;
