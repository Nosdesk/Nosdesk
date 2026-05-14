-- Inverse of 2026-05-14-200000. Re-adds the columns to `users`,
-- copies preferences back, then drops `user_preferences`. Loses
-- any prefs that landed AFTER the downgrade window because of
-- the standard "down migrations don't preserve forward data"
-- expectation.

ALTER TABLE users
    ADD COLUMN theme VARCHAR(50),
    ADD COLUMN signature TEXT,
    ADD COLUMN dashboard_layout JSONB,
    ADD COLUMN locale TEXT,
    ADD COLUMN timezone TEXT;

UPDATE users u SET
    theme = p.theme,
    signature = p.signature,
    dashboard_layout = p.dashboard_layout,
    locale = p.locale,
    timezone = p.timezone
FROM user_preferences p
WHERE u.uuid = p.user_uuid;

DROP TRIGGER IF EXISTS trg_users_auto_create_preferences ON users;
DROP FUNCTION IF EXISTS auto_create_user_preferences();
DROP TABLE user_preferences;
