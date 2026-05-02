-- The projects_v2 feature flag is gone — the V2 sync-engine views
-- are now the only implementation, the legacy REST views were
-- deleted alongside the dispatchers that gated them. Strip the
-- workspace-level flag and any per-user override so the
-- feature_flags JSONB doesn't carry a key nothing reads.

UPDATE site_settings
   SET feature_flags = feature_flags - 'projects_v2'
 WHERE id = 1;

UPDATE users
   SET feature_flag_overrides = feature_flag_overrides - 'projects_v2'
 WHERE feature_flag_overrides ? 'projects_v2';
