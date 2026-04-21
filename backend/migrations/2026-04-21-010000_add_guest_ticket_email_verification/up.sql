-- When true (default), a confirmation / sign-in invitation email is sent to
-- the submitter each time a new guest user is provisioned through the public
-- ticket form. When false, no email is sent and the submitter can only track
-- their ticket via the optional guest lookup token.
ALTER TABLE site_settings
    ADD COLUMN guest_ticket_email_verification BOOLEAN NOT NULL DEFAULT true;
