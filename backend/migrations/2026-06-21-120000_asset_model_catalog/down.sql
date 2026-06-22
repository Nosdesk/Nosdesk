-- Reverse the asset model catalog.

DROP INDEX IF EXISTS public.idx_assets_model;
ALTER TABLE public.assets DROP CONSTRAINT IF EXISTS assets_model_id_fkey;
ALTER TABLE public.assets DROP COLUMN IF EXISTS model_id;

DROP TABLE IF EXISTS public.asset_models;
DROP TABLE IF EXISTS public.manufacturers;
