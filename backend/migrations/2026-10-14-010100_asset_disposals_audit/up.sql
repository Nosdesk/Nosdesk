-- A disposal record is the compliance record of how an asset was sanitised
-- and who took it. Audit every change to it, and keep it append-only for the
-- app role: nothing in the app updates or deletes one. Deleting the asset or
-- the workspace still removes it, and purging a user still clears actor_uuid,
-- because foreign-key actions run as the table owner.
CREATE TRIGGER tr_audit_asset_disposals AFTER INSERT OR DELETE OR UPDATE ON public.asset_disposals
    FOR EACH ROW EXECUTE FUNCTION public.audit_log_trigger('id');

REVOKE UPDATE, DELETE ON TABLE public.asset_disposals FROM nosdesk_app;
