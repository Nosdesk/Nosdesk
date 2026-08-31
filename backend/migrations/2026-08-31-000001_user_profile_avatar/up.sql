-- Per-workspace avatar override. Mirrors user_profiles.display_name (O7): the
-- product owns the per-workspace profile, so this stays clear of the control
-- plane's re-projection of the global users.avatar_url. NULL means fall back to
-- the global avatar.
ALTER TABLE user_profiles ADD COLUMN avatar_url VARCHAR(2048);
