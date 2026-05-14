ALTER TABLE site_settings
    DROP COLUMN default_timezone,
    DROP COLUMN default_locale;

ALTER TABLE users
    DROP COLUMN timezone,
    DROP COLUMN locale;
