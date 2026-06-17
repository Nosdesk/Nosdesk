-- Outbound mail class: notification vs transactional.
--
-- Deliverability features branch on this. List-Unsubscribe + One-Click (B2)
-- goes only on notification mail (ticket-update notifications, the opt-out-able
-- class), never on transactional mail (password reset, invitation, the agent's
-- reply, auto-ack). It is a distinct axis from sender_identity: a conversation
-- reply is workspace-identity but transactional; a notification is
-- workspace-identity but notification. The two classes are otherwise
-- indistinguishable in the queue (notifications carry no ticket/channel/comment
-- id either), so record the policy explicitly at enqueue and let the worker and
-- the B-items branch on it rather than re-deriving it.
--
-- ADD COLUMN ... DEFAULT is metadata-only in PG11+ (no row rewrite, no per-row
-- audit-trigger fire). 'transactional' is the safe default: it omits
-- List-Unsubscribe, so an unclassified row is never treated as opt-out-able.

ALTER TABLE public.outbound_emails
    ADD COLUMN mail_class character varying(16) NOT NULL DEFAULT 'transactional';

ALTER TABLE public.outbound_emails
    ADD CONSTRAINT outbound_emails_mail_class_check
    CHECK (mail_class::text = ANY (ARRAY['transactional'::text, 'notification'::text]));
