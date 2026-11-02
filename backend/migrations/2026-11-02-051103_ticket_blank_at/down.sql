DROP INDEX IF EXISTS public.tickets_blank_by_requester;
ALTER TABLE public.tickets DROP COLUMN IF EXISTS blank_at;
