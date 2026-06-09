DROP TABLE IF EXISTS public.asset_media;
DROP SEQUENCE IF EXISTS public.asset_media_id_seq;

-- Postgres cannot drop individual enum labels safely; reversing the
-- `asset_media` sync_aggregate label would require rebuilding the type
-- and rewriting every column that references it.
