-- The acknowledgement a requester gets when a request is filed for them (the
-- portal, the request form, the API). Mail that opens a ticket has its own
-- pair, channel_auto_ack_enabled and channel_auto_ack_template. On by default,
-- as it always was; a NULL template is the built-in wording.
ALTER TABLE public.site_settings
    ADD COLUMN requester_ack_enabled boolean NOT NULL DEFAULT true,
    ADD COLUMN requester_ack_template text;

-- When the address last proved itself: a sign-in link or code, or a POST that
-- confirms it. Following a "View request" link is not proof (mail scanners
-- open every link). NULL for an address that has never proved itself since
-- this column was added. Mail about a ticket a stranger filed carries the
-- ticket's own text only once the requester has proved the address since.
ALTER TABLE public.user_emails
    ADD COLUMN proven_at timestamptz;
