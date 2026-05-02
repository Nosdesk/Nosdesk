-- Reverse: remove the projects_v2 default from the workspace
-- feature flags. Per-user overrides survive.

UPDATE site_settings
   SET feature_flags = feature_flags - 'projects_v2'
 WHERE id = 1;
