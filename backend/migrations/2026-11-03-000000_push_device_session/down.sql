-- The registrations dropped on the way up stay dropped; the app registers
-- again on its next launch.
ALTER TABLE public.user_push_devices DROP COLUMN session_id;
