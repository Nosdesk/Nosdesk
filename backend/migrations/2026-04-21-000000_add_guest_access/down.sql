DROP INDEX IF EXISTS idx_tickets_guest_lookup_token;

ALTER TABLE tickets
    DROP COLUMN IF EXISTS submitted_via,
    DROP COLUMN IF EXISTS guest_lookup_token;

ALTER TABLE site_settings
    DROP COLUMN IF EXISTS guest_tickets_enabled,
    DROP COLUMN IF EXISTS guest_public_docs_enabled,
    DROP COLUMN IF EXISTS guest_kb_search_enabled,
    DROP COLUMN IF EXISTS guest_ticket_lookup_enabled,
    DROP COLUMN IF EXISTS guest_help_page_enabled,
    DROP COLUMN IF EXISTS guest_ticket_default_priority,
    DROP COLUMN IF EXISTS guest_ticket_rate_limit_per_hour;
