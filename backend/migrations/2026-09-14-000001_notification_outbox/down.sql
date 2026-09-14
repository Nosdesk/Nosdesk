DROP INDEX IF EXISTS public.notifications_source_sync_uq;
ALTER TABLE public.notifications DROP COLUMN IF EXISTS source_sync_id;
DROP TRIGGER IF EXISTS tr_sync_actions_notification_outbox ON public.sync_actions;
DROP FUNCTION IF EXISTS public.notification_outbox_enqueue();
DROP TABLE IF EXISTS public.notification_outbox;
