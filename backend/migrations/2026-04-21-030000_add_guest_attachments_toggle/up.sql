-- Kill-switch for the public guest upload endpoint. Kept separate from
-- guest_tickets_enabled so admins can accept text-only submissions if
-- attachment abuse becomes a problem without disabling the whole feature.
ALTER TABLE site_settings
    ADD COLUMN guest_ticket_attachments_enabled BOOLEAN NOT NULL DEFAULT true;
