-- Per-workspace site_settings.
--
-- site_settings already carried a workspace_id column + an RLS isolation
-- policy, but a `CHECK (id = 1)` constraint and a static `id DEFAULT 1`
-- primary key physically capped the table at a single row. Every
-- workspace's settings collapsed onto that one id=1 row (workspace 1), so
-- under hosted RLS any other workspace's read is filtered out: 500 on
-- /api/feature-flags, silent default branding/guest config.
--
-- Lift the single-row cap so the workspace_id column + RLS can actually be
-- used: drop the id=1 CHECK, back id with a sequence, and enforce one
-- settings row per workspace with UNIQUE(workspace_id). Then backfill a row
-- for every existing workspace that lacks one.

ALTER TABLE public.site_settings DROP CONSTRAINT site_settings_id_check;

CREATE SEQUENCE IF NOT EXISTS public.site_settings_id_seq;
-- The sequence must share the table's owner for OWNED BY; the table is
-- owned by nosdesk_admin (role-name-agnostic baseline owner). nosdesk_app
-- (the runtime role) inserts settings rows via lazy-create, so it needs
-- sequence usage.
ALTER SEQUENCE public.site_settings_id_seq OWNER TO nosdesk_admin;
ALTER SEQUENCE public.site_settings_id_seq OWNED BY public.site_settings.id;
GRANT USAGE, SELECT ON SEQUENCE public.site_settings_id_seq TO nosdesk_app;
SELECT setval('public.site_settings_id_seq', GREATEST((SELECT max(id) FROM public.site_settings), 1));
ALTER TABLE public.site_settings ALTER COLUMN id SET DEFAULT nextval('public.site_settings_id_seq');

ALTER TABLE public.site_settings
    ADD CONSTRAINT site_settings_workspace_id_key UNIQUE (workspace_id);

-- One default settings row per workspace that doesn't have one (workspaces
-- provisioned before per-workspace settings). The column defaults populate
-- every field; workspace_id is set explicitly because the GUC-based column
-- default doesn't apply in this migration/admin session.
INSERT INTO public.site_settings (workspace_id)
SELECT w.id
FROM public.workspaces w
WHERE NOT EXISTS (
    SELECT 1 FROM public.site_settings s WHERE s.workspace_id = w.id
);
