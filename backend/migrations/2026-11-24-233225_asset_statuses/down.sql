DROP INDEX IF EXISTS public.asset_loans_status_id_before_idx;
ALTER TABLE public.asset_loans DROP CONSTRAINT IF EXISTS asset_loans_status_before_category_fkey;
ALTER TABLE public.asset_loans DROP CONSTRAINT IF EXISTS asset_loans_status_id_before_fkey;
ALTER TABLE public.asset_loans DROP COLUMN IF EXISTS status_id_before;

DROP INDEX IF EXISTS public.asset_lifecycle_events_to_status_id_idx;
DROP INDEX IF EXISTS public.asset_lifecycle_events_from_status_id_idx;
ALTER TABLE public.asset_lifecycle_events
    DROP CONSTRAINT IF EXISTS asset_lifecycle_events_to_status_category_fkey,
    DROP CONSTRAINT IF EXISTS asset_lifecycle_events_to_status_id_fkey,
    DROP CONSTRAINT IF EXISTS asset_lifecycle_events_from_status_category_fkey,
    DROP CONSTRAINT IF EXISTS asset_lifecycle_events_from_status_id_fkey;
ALTER TABLE public.asset_lifecycle_events
    DROP COLUMN IF EXISTS to_status_id,
    DROP COLUMN IF EXISTS from_status_id;

DROP INDEX IF EXISTS public.assets_status_id_idx;
ALTER TABLE public.assets DROP CONSTRAINT IF EXISTS assets_status_category_fkey;
ALTER TABLE public.assets DROP CONSTRAINT IF EXISTS assets_status_id_fkey;
ALTER TABLE public.assets DROP COLUMN IF EXISTS status_id;

DROP TABLE IF EXISTS public.asset_statuses;
