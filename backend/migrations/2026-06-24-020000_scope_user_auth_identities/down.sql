DROP INDEX IF EXISTS public.idx_user_auth_identities_workspace_id;
DROP INDEX IF EXISTS public.user_auth_identities_scoped_uq;
DROP INDEX IF EXISTS public.user_auth_identities_global_uq;
ALTER TABLE public.user_auth_identities
    ADD CONSTRAINT user_auth_identities_provider_type_external_id_key UNIQUE (provider_type, external_id);
ALTER TABLE public.user_auth_identities
    DROP CONSTRAINT user_auth_identities_workspace_id_fkey;
ALTER TABLE public.user_auth_identities
    DROP COLUMN workspace_id;
