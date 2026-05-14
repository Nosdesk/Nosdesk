-- Split user-facing display preferences out of `users` into a
-- dedicated `user_preferences` table.
--
-- Why: `users` had accumulated theme, signature, dashboard_layout,
-- locale, and timezone — five preference-ish columns, past the
-- Mastodon "2-3 fields directly on users" threshold. Splitting
-- keeps the high-churn user row narrow, lets future preferences
-- land in one obvious place, and groups related concerns
-- together for the response layer.
--
-- NOT moved (remain on `users`):
--   - mfa_secret / mfa_enabled / mfa_backup_codes — auth, not prefs
--   - feature_flag_overrides — admin-set override, not user pref
--   - microsoft_uuid — identity, not pref
--
-- Lifecycle: a row in `user_preferences` exists for every user.
-- A BEFORE INSERT trigger on `users` materialises it on creation,
-- and the FK is `ON DELETE CASCADE` so user deletion drops the
-- row automatically. NULL columns mean "use the system default
-- from site_settings".

CREATE TABLE user_preferences (
    user_uuid    UUID         PRIMARY KEY REFERENCES users(uuid) ON DELETE CASCADE,
    theme        VARCHAR(50),
    signature    TEXT,
    dashboard_layout JSONB,
    locale       TEXT,
    timezone     TEXT,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ  NOT NULL DEFAULT now()
);

COMMENT ON TABLE  user_preferences IS
    'Per-user UI and display preferences. Split from `users` once the preference set exceeded the Mastodon "few-columns-on-users" threshold. A row exists for every user (auto-created via trigger); NULL columns mean "fall back to site_settings.default_*".';
COMMENT ON COLUMN user_preferences.theme IS
    'UI theme name (e.g. light, dark, system). NULL = system default.';
COMMENT ON COLUMN user_preferences.signature IS
    'Free-form text appended to outbound channel replies. NULL / empty = no signature.';
COMMENT ON COLUMN user_preferences.dashboard_layout IS
    'JSONB shape `{ widgets: [{ id, visible }] }` driving the dashboard widget order and visibility. NULL = role default (computed client-side).';
COMMENT ON COLUMN user_preferences.locale IS
    'User preferred locale (BCP-47, e.g. en-US). NULL = inherit from site_settings.default_locale.';
COMMENT ON COLUMN user_preferences.timezone IS
    'User preferred IANA timezone (e.g. Europe/Berlin). Offsets are not accepted (no DST encoding). NULL = inherit from site_settings.default_timezone.';

-- Trigger: auto-create the prefs row on every new user. KISS:
-- one place, can't be forgotten by application code paths that
-- create users (and there are many — channels ingest for guests,
-- admin invite, OAuth callback, etc.). The trigger is BEFORE
-- INSERT only; updates / deletes flow through the FK + the
-- application's own update path.
CREATE OR REPLACE FUNCTION auto_create_user_preferences()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO user_preferences (user_uuid)
    VALUES (NEW.uuid)
    ON CONFLICT (user_uuid) DO NOTHING;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_users_auto_create_preferences
    AFTER INSERT ON users
    FOR EACH ROW
    EXECUTE FUNCTION auto_create_user_preferences();

-- Backfill: every existing user gets a prefs row carrying the
-- values that were previously on the users row. `ON CONFLICT DO
-- NOTHING` covers the race where the trigger just fired for the
-- same uuid (shouldn't happen during a one-transaction migration
-- but the guard is essentially free).
INSERT INTO user_preferences (user_uuid, theme, signature, dashboard_layout, locale, timezone)
SELECT uuid, theme, signature, dashboard_layout, locale, timezone
FROM users
ON CONFLICT (user_uuid) DO NOTHING;

-- Drop the moved columns. Done last so backfill has data to read
-- from. Diesel will pick up the schema change in the next
-- print-schema run.
ALTER TABLE users
    DROP COLUMN theme,
    DROP COLUMN signature,
    DROP COLUMN dashboard_layout,
    DROP COLUMN locale,
    DROP COLUMN timezone;
