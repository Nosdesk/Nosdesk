DROP INDEX IF EXISTS public.comments_ticket_author_client_id_key;
ALTER TABLE public.comments DROP COLUMN IF EXISTS client_id;
