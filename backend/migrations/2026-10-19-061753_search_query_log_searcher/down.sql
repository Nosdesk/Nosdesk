DROP INDEX IF EXISTS public.idx_search_query_log_user_uuid;
ALTER TABLE public.search_query_log DROP COLUMN IF EXISTS user_uuid;
