ALTER TABLE public.user_profiles DROP COLUMN IF EXISTS manager_uuid;
DROP TABLE IF EXISTS public.category_approvers;
ALTER TABLE public.ticket_categories
    DROP COLUMN IF EXISTS approval_by_manager,
    DROP COLUMN IF EXISTS approval_rule,
    DROP COLUMN IF EXISTS approval_required;
ALTER TABLE public.site_settings
    DROP COLUMN IF EXISTS approval_auto_approve_days,
    DROP COLUMN IF EXISTS approval_skip_by,
    DROP COLUMN IF EXISTS approval_waiting_display;
