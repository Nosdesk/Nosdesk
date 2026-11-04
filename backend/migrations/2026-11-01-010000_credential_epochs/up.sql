-- How many times each account has been signed out everywhere. Stateless
-- bearers that outlive a session (the portal's "View request" and approval
-- links) sign the value they were issued under, and a revocation of every
-- session raises it, so those links stop working too. No row means 0.
--
-- Global like users and active_sessions: an account's sign-in state isn't a
-- tenant's, so there is no RLS. Kept off the audited users row because a
-- revocation runs where no workspace is pinned (the operator API, the CLI);
-- security_events records each one.
CREATE TABLE public.credential_epochs (
    user_uuid  uuid        PRIMARY KEY REFERENCES public.users (uuid) ON DELETE CASCADE,
    epoch      integer     NOT NULL DEFAULT 0,
    raised_at  timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE public.credential_epochs OWNER TO nosdesk_admin;
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.credential_epochs TO nosdesk_app;
