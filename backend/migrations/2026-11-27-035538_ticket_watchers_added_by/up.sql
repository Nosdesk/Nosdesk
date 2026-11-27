-- Who put a person on a request. Mail about a request carries its text to
-- someone outside the team only when the team put them there, or they have
-- written on it themselves (services::ticket_mail_redaction). The default is
-- the acting user the write runs as; a system write (inbound mail copying in
-- a Cc) leaves it NULL, as every row from before this column does. No
-- foreign key, like outbound_emails.skip_if_seen_by: a second one to users
-- would leave the watcher-to-user join ambiguous.
ALTER TABLE public.ticket_watchers
    ADD COLUMN added_by uuid;
ALTER TABLE public.ticket_watchers
    ALTER COLUMN added_by SET DEFAULT (NULLIF(current_setting('app.actor_uuid', true), ''))::uuid;
