-- A push registration belongs to the signed-in session that made it and goes
-- with it, however that session ends: signing out, signing out everywhere, a
-- password reset, eviction at the session cap or expiry. The device couldn't
-- reliably revoke it at sign-out itself: that needs the device online, and a
-- revoked session's bearer can no longer authenticate the request.
ALTER TABLE public.user_push_devices ADD COLUMN session_id uuid;

-- Existing registrations predate the link. Each goes to its person's most
-- recently active live session from the same kind of device (the app names
-- its sessions "iPhone (…)" or "Android (…)"). With no such session, no device
-- of that kind is signed in, so the registration was left behind by a
-- sign-out and goes. The app registers again on every launch, which corrects
-- a match made between two of a person's phones.
UPDATE public.user_push_devices d
SET session_id = (
    SELECT s.session_id
    FROM public.active_sessions s
    WHERE s.user_uuid = d.user_uuid
      AND s.expires_at > now()
      AND s.device_name LIKE CASE d.platform
          WHEN 'ios' THEN 'iPhone%'
          WHEN 'android' THEN 'Android%'
      END
    ORDER BY s.last_active DESC
    LIMIT 1
);
DELETE FROM public.user_push_devices WHERE session_id IS NULL;

ALTER TABLE public.user_push_devices
    ALTER COLUMN session_id SET NOT NULL,
    ADD CONSTRAINT user_push_devices_session_id_fkey FOREIGN KEY (session_id)
        REFERENCES public.active_sessions(session_id) ON DELETE CASCADE;

CREATE INDEX idx_user_push_devices_session_id ON public.user_push_devices (session_id);
