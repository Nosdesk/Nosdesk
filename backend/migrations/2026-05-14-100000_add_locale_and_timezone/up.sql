-- Locale + timezone preferences, per user and site-wide.
--
-- Resolution chain (read by `utils::locale::resolve_locale` /
-- `resolve_timezone`):
--   1. users.locale   / users.timezone   (NULL = unset, fall through)
--   2. site_settings.default_locale / site_settings.default_timezone
--   3. hardcoded fallback ('en-US' / 'UTC') if site_settings is
--      somehow missing rows (shouldn't happen in practice — the row
--      is seeded by first-run; this is the belt-and-braces case)
--
-- Locale values are BCP-47 (e.g. 'en-US', 'en-GB', 'en-AU', 'de-DE'),
-- with hyphens not underscores — what `Intl` APIs and HTTP
-- `Accept-Language` use. Timezone values are IANA TZDB identifiers
-- (e.g. 'Europe/Berlin'), never offsets.

ALTER TABLE users
    ADD COLUMN locale TEXT,
    ADD COLUMN timezone TEXT;

COMMENT ON COLUMN users.locale IS
    'User preferred locale (BCP-47, e.g. en-US). NULL = inherit from site_settings.default_locale.';
COMMENT ON COLUMN users.timezone IS
    'User preferred IANA timezone (e.g. Europe/Berlin). Offsets are not accepted (no DST encoding). NULL = inherit from site_settings.default_timezone.';

ALTER TABLE site_settings
    ADD COLUMN default_locale TEXT NOT NULL DEFAULT 'en-US',
    ADD COLUMN default_timezone TEXT NOT NULL DEFAULT 'UTC';

COMMENT ON COLUMN site_settings.default_locale IS
    'System-wide fallback locale (BCP-47) used when a user has no preference. Also used for outbound mail to guests whose Content-Language was missing or unsupported.';
COMMENT ON COLUMN site_settings.default_timezone IS
    'System-wide fallback IANA timezone used when a user has no preference.';
