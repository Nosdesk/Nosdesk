-- Remove the backfilled repair records. The repair types it added stay: a
-- workspace may have used them since, and an unused one is harmless. A file
-- linked to a removed record loses the link, which writes asset_media, so
-- its audit trigger is off too (NDX01).
ALTER TABLE public.asset_records DISABLE TRIGGER tr_audit_asset_records;
ALTER TABLE public.asset_media DISABLE TRIGGER tr_audit_asset_media;
DELETE FROM public.asset_records WHERE source = 'backfill';
ALTER TABLE public.asset_media ENABLE TRIGGER tr_audit_asset_media;
ALTER TABLE public.asset_records ENABLE TRIGGER tr_audit_asset_records;
