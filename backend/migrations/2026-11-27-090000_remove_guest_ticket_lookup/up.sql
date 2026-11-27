-- The guest status lookup is gone: no guest receives a lookup link since the
-- request form stopped answering with one, so its toggle and its per-ticket
-- token served nothing. Dropping the column drops its unique constraint and
-- index with it.
ALTER TABLE public.site_settings DROP COLUMN IF EXISTS guest_ticket_lookup_enabled;
ALTER TABLE public.tickets DROP COLUMN IF EXISTS guest_lookup_token;
