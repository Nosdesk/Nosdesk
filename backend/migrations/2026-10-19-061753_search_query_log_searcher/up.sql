-- Who ran each logged search, so the failed-search detector can tell one
-- person's typing chain ("Sea", "Sear", "Search") from separate searches and
-- count the people a missing doc affects. Rows logged before this column
-- existed keep NULL. A person's searches go when they are purged.
-- DDL only: no rows change, and search_query_log has no audit trigger.
ALTER TABLE public.search_query_log
    ADD COLUMN user_uuid uuid REFERENCES public.users(uuid) ON DELETE CASCADE;

CREATE INDEX idx_search_query_log_user_uuid
    ON public.search_query_log USING btree (user_uuid)
    WHERE user_uuid IS NOT NULL;
