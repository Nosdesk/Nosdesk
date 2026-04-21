-- Guest access controls on site_settings
ALTER TABLE site_settings
    ADD COLUMN guest_tickets_enabled BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN guest_public_docs_enabled BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN guest_kb_search_enabled BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN guest_ticket_lookup_enabled BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN guest_help_page_enabled BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN guest_ticket_default_priority VARCHAR(32),
    ADD COLUMN guest_ticket_rate_limit_per_hour INTEGER NOT NULL DEFAULT 5;

-- Provenance + opaque lookup token for public status page
ALTER TABLE tickets
    ADD COLUMN submitted_via VARCHAR(32),
    ADD COLUMN guest_lookup_token UUID UNIQUE;

CREATE INDEX idx_tickets_guest_lookup_token ON tickets(guest_lookup_token)
    WHERE guest_lookup_token IS NOT NULL;
