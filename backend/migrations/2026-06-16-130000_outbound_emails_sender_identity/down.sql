-- Dropping the column removes its check constraint too.
ALTER TABLE public.outbound_emails DROP COLUMN IF EXISTS sender_identity;
