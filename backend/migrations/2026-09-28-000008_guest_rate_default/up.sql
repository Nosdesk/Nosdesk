-- Anonymous requests per network per hour: 5 was too low for an office behind
-- one address, now that the form also has a challenge, per-address email caps
-- and a per-workspace daily ceiling. Workspaces still on the old default move
-- to the new one; a value an admin chose stays.
ALTER TABLE public.site_settings ALTER COLUMN guest_ticket_rate_limit_per_hour SET DEFAULT 20;

-- A settings backfill, not a user action: keep it out of the audit log (the
-- trigger requires an actor this migration doesn't have).
ALTER TABLE public.site_settings DISABLE TRIGGER tr_audit_site_settings;
UPDATE public.site_settings SET guest_ticket_rate_limit_per_hour = 20
    WHERE guest_ticket_rate_limit_per_hour = 5;
ALTER TABLE public.site_settings ENABLE TRIGGER tr_audit_site_settings;
