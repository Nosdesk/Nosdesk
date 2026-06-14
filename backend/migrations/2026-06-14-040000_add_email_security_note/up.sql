-- Configurable anti-phishing footer note for transactional emails.
-- Mirrors the channel_auto_ack pair: a toggle plus an optional
-- admin-authored template. Off by default; NULL template = use the
-- built-in localized default (FTL key email-security-note-default).
ALTER TABLE site_settings
    ADD COLUMN email_security_note_enabled BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN email_security_note_template TEXT;
