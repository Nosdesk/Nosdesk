-- Requester SSO (docs/plans/requester-phase3.md, item 5): a workspace's own
-- OpenID Connect provider for portal sign-in (Microsoft Entra ID, Google
-- Workspace or any OIDC IdP). One per workspace. Staff sign-in is separate.
--
-- The client secret is stored KEK-encrypted like the LDAP bind password (framed
-- AES-256-GCM, workspace id bound into the AAD, kek_id sidecar). Only emails at
-- `allowed_domains` sign in or join through it.
CREATE TABLE public.workspace_identity_providers (
    id                      serial      PRIMARY KEY,
    kind                    text        NOT NULL CHECK (kind IN ('entra', 'google', 'oidc')),
    display_name            varchar(80) NOT NULL,
    issuer_url              text        NOT NULL,
    client_id               text        NOT NULL,
    encrypted_client_secret bytea,
    encrypted_kek_id        smallint,
    allowed_domains         text[]      NOT NULL DEFAULT '{}',
    enabled                 boolean     NOT NULL DEFAULT false,
    workspace_id            integer     NOT NULL UNIQUE
                            DEFAULT (NULLIF(current_setting('app.workspace_id', true), ''))::integer
                            REFERENCES public.workspaces(id) ON DELETE CASCADE,
    created_at              timestamptz NOT NULL DEFAULT now(),
    updated_at              timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE public.workspace_identity_providers OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, DELETE, UPDATE ON TABLE public.workspace_identity_providers TO nosdesk_app;
GRANT USAGE, SELECT ON SEQUENCE public.workspace_identity_providers_id_seq TO nosdesk_app;

ALTER TABLE public.workspace_identity_providers ENABLE ROW LEVEL SECURITY;
ALTER TABLE public.workspace_identity_providers FORCE ROW LEVEL SECURITY;
CREATE POLICY workspace_identity_providers_workspace_isolation ON public.workspace_identity_providers
    USING (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer)
    WITH CHECK (workspace_id = (NULLIF(current_setting('app.workspace_id', true), ''))::integer);

CREATE TRIGGER tr_audit_workspace_identity_providers
    AFTER INSERT OR DELETE OR UPDATE ON public.workspace_identity_providers
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id', 'encrypted_client_secret');
