-- The workspace's currency: the ISO 4217 code money fields offer by default.
-- Null until an admin picks one; clients then suggest one from the
-- workspace's default locale. Every money value stores its own code, so a
-- change here never reinterprets an amount already saved. Adding a nullable
-- column rewrites no rows, so the site_settings audit trigger doesn't fire.
ALTER TABLE public.site_settings
    ADD COLUMN currency_code char(3)
        CONSTRAINT site_settings_currency_code_check
        CHECK (currency_code ~ '^[A-Z]{3}$');
