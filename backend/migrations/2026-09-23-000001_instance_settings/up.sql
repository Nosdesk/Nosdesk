-- Instance-wide settings an operator changes from the admin UI instead of the
-- environment (docs/plans/self-hosted-license-activation.md). One row.
--
-- Not a tenant table: no workspace_id, no RLS. Environment variables win over
-- every column here (NOSDESK_LICENSE_KEY, NOSDESK_PUSH_MODE), so a deployment
-- managed through env behaves exactly as before.

CREATE TABLE public.instance_settings (
    id                          boolean     PRIMARY KEY DEFAULT true CHECK (id),
    -- The licence token, AES-GCM framed by utils::encryption (AAD
    -- "instance_settings.license_key"). It is signed rather than secret, but it
    -- is also the relay credential, so it gets the same custody as an SMTP
    -- password.
    license_key_encrypted       bytea,
    -- Mirror of the KEK version inside the blob, like every other encrypted
    -- column, so a key rotation can find what it has to rewrap.
    license_key_kek_id          smallint,
    -- pasted | linked
    license_source              varchar(16) CHECK (license_source IN ('pasted', 'linked')),
    license_installed_at        timestamptz,
    license_installed_by        uuid,
    license_last_refresh_at     timestamptz,
    -- A static error kind, never a message body.
    license_last_refresh_error  text,
    -- native | relay | off; NULL means "decide from the environment" (the
    -- historical behaviour).
    push_mode                   varchar(16) CHECK (push_mode IN ('native', 'relay', 'off')),
    updated_at                  timestamptz NOT NULL DEFAULT now(),
    CHECK ((license_key_encrypted IS NULL) = (license_source IS NULL)),
    CHECK ((license_key_encrypted IS NULL) = (license_key_kek_id IS NULL))
);
ALTER TABLE public.instance_settings OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.instance_settings TO nosdesk_app;
