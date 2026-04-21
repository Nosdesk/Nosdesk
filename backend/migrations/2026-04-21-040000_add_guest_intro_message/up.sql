-- Optional intro blurb shown at the top of the public guest ticket form.
-- Plain text only (rendered with preserved line breaks on the frontend —
-- no markdown, no HTML, no XSS surface). Null means "no intro."
ALTER TABLE site_settings
    ADD COLUMN guest_ticket_intro_message TEXT;
