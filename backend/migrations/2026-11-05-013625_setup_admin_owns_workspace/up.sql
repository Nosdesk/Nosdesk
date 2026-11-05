-- First-run setup now makes its administrator the owner of the bootstrap
-- workspace (services::admin_setup). Before, it made them an admin, so an
-- install set up earlier has no owner there, and the owner-only workspace
-- export and import are open to no one in the app.
--
-- Where the bootstrap workspace has no current owner, its setup
-- administrator becomes one: the earliest platform admin who is a current
-- admin of it and whose account is live. Setup is the only grant of the
-- platform admin role, so that is who ran it. Hosted has no platform admins
-- and its workspaces have owners, so nothing changes there. The membership
-- audit trigger is held off: the migration history is the record, and a
-- restored backup upgrades to the same rows as an install upgraded in place.
ALTER TABLE public.workspace_members DISABLE TRIGGER tr_audit_workspace_members;

UPDATE public.workspace_members m
SET role = 'owner'
WHERE m.workspace_id = 1
  AND NOT EXISTS (
    SELECT 1 FROM public.workspace_members o
    WHERE o.workspace_id = 1
      AND o.role = 'owner'
      AND o.removed_at IS NULL
  )
  AND m.user_uuid = (
    SELECT a.user_uuid
    FROM public.workspace_members a
    JOIN public.users u ON u.uuid = a.user_uuid
    WHERE a.workspace_id = 1
      AND a.role = 'admin'
      AND a.removed_at IS NULL
      AND u.platform_role = 'platform_admin'
      AND u.deleted_at IS NULL
    ORDER BY u.created_at, u.uuid
    LIMIT 1
  );

ALTER TABLE public.workspace_members ENABLE TRIGGER tr_audit_workspace_members;
