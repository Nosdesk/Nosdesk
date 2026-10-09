DROP TRIGGER IF EXISTS ticket_raised_by_staff ON public.tickets;
DROP FUNCTION IF EXISTS public.ticket_raised_by_staff();
ALTER TABLE public.tickets DROP COLUMN IF EXISTS raised_by_staff;
ALTER TABLE public.site_settings DROP COLUMN IF EXISTS portal_share_staff_requests;
