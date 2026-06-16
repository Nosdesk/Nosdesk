-- Restore the single-row cap. Best-effort: the CHECK only re-applies if a
-- single row (id=1) remains, so this is valid only right after a fresh
-- up-migration, not after multiple workspaces have settings rows.
ALTER TABLE public.site_settings DROP CONSTRAINT site_settings_workspace_id_key;
ALTER TABLE public.site_settings ALTER COLUMN id DROP DEFAULT;
DROP SEQUENCE IF EXISTS public.site_settings_id_seq;
ALTER TABLE public.site_settings ADD CONSTRAINT site_settings_id_check CHECK ((id = 1));
