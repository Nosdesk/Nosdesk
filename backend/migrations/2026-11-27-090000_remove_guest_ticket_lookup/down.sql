ALTER TABLE public.tickets ADD COLUMN IF NOT EXISTS guest_lookup_token uuid;
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'tickets_guest_lookup_token_key'
          AND conrelid = 'public.tickets'::regclass
    ) THEN
        ALTER TABLE public.tickets
            ADD CONSTRAINT tickets_guest_lookup_token_key UNIQUE (guest_lookup_token);
    END IF;
END
$$;
CREATE INDEX IF NOT EXISTS idx_tickets_guest_lookup_token
    ON public.tickets USING btree (guest_lookup_token)
    WHERE (guest_lookup_token IS NOT NULL);
ALTER TABLE public.site_settings
    ADD COLUMN IF NOT EXISTS guest_ticket_lookup_enabled boolean DEFAULT false NOT NULL;
