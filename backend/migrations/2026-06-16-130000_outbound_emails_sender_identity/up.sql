-- Outbound mail identity routing.
--
-- The queue worker resolves a per-workspace SMTP identity at send time, but
-- auth/platform mail (password reset, invitation) must keep sending from the
-- instance identity, not a tenant's relay (a phishing + deliverability
-- guard). Notification and conversation mail use the workspace identity.
-- These classes are otherwise indistinguishable in the queue (notifications
-- carry no ticket/channel/comment id either), so record the policy
-- explicitly at enqueue and let the worker route on it.
--
-- ADD COLUMN ... DEFAULT is metadata-only in PG11+ (no row rewrite, no
-- per-row audit-trigger fire). The 'workspace' default is safe at deploy:
-- no workspace identity exists until an admin opts in, so every row falls
-- back to the platform identity in the meantime regardless of this value.

ALTER TABLE public.outbound_emails
    ADD COLUMN sender_identity character varying(16) NOT NULL DEFAULT 'workspace';

ALTER TABLE public.outbound_emails
    ADD CONSTRAINT outbound_emails_sender_identity_check
    CHECK (sender_identity::text = ANY (ARRAY['workspace'::text, 'platform'::text]));
