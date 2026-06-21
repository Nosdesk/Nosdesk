-- Ticket-level spam-suspected flag (hot: read on every queue row for the badge).
--
-- Set true when a ticket opens from inbound mail the provider flagged as spam
-- (SES X-SES-Spam-Verdict on the forwarding path). We never drop the mail; the
-- ticket opens flagged + low-priority so agents can triage it from the queue.
-- Clearing the flag ("not spam") is a normal ticket update.
--
-- Room for this came from normalizing the cold merge columns out into
-- ticket_merges in the previous migration. ADD COLUMN ... DEFAULT is
-- metadata-only in PG11+ (no row rewrite, no per-row audit-trigger fire); the
-- audit trigger captures the row generically, so no trigger recreation needed.

ALTER TABLE public.tickets
    ADD COLUMN spam_suspected boolean NOT NULL DEFAULT false;
