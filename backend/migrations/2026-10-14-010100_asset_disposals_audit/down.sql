GRANT UPDATE, DELETE ON TABLE public.asset_disposals TO nosdesk_app;
DROP TRIGGER IF EXISTS tr_audit_asset_disposals ON public.asset_disposals;
