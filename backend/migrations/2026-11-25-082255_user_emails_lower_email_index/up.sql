-- Every lookup of an address compares it case-insensitively, so the directory
-- syncs and the address writers match on lower(email). This index serves
-- those matches without a scan of every address.
CREATE INDEX IF NOT EXISTS idx_user_emails_lower_email ON public.user_emails (lower(email));
