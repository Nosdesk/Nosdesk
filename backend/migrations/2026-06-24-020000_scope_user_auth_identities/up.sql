-- Pre-P0 (LDAP/directory integration): scope auth identities by workspace so the
-- same directory external_id (entryUUID/objectGUID, SCIM externalId) can exist in
-- more than one workspace. Global login identities (local, microsoft, oidc) keep
-- workspace_id NULL and stay unique on (provider_type, external_id); directory
-- identities (ldap, scim) set workspace_id and are unique within their workspace.
--
-- The table has no audit trigger and no RLS, so existing rows simply default to
-- NULL (global) with no backfill needed; login/sync lookups are unchanged.

ALTER TABLE public.user_auth_identities
    ADD COLUMN workspace_id integer;
ALTER TABLE public.user_auth_identities
    ADD CONSTRAINT user_auth_identities_workspace_id_fkey
    FOREIGN KEY (workspace_id) REFERENCES public.workspaces(id) ON DELETE CASCADE;

-- Replace the single global unique with two partial uniques.
ALTER TABLE public.user_auth_identities
    DROP CONSTRAINT user_auth_identities_provider_type_external_id_key;
CREATE UNIQUE INDEX user_auth_identities_global_uq
    ON public.user_auth_identities (provider_type, external_id)
    WHERE workspace_id IS NULL;
CREATE UNIQUE INDEX user_auth_identities_scoped_uq
    ON public.user_auth_identities (workspace_id, provider_type, external_id)
    WHERE workspace_id IS NOT NULL;
CREATE INDEX idx_user_auth_identities_workspace_id
    ON public.user_auth_identities (workspace_id);
