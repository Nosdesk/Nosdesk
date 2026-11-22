DROP INDEX IF EXISTS public.asset_media_record_idx;
ALTER TABLE public.asset_media DROP CONSTRAINT IF EXISTS asset_media_record_id_fkey;
ALTER TABLE public.asset_media DROP COLUMN IF EXISTS record_id;
DROP TABLE IF EXISTS public.asset_records;
DROP TABLE IF EXISTS public.asset_record_types;
