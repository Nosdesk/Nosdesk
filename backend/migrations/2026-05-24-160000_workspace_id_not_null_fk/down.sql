-- Reverse the workspace_id NOT NULL + FK + default-from-GUC
-- migration. Restores Phase 1's nullable column with DEFAULT 1.

DO $$
DECLARE
    t text;
    fk_name text;
BEGIN
    FOR t IN
        SELECT table_name
        FROM information_schema.columns
        WHERE column_name = 'workspace_id'
          AND table_schema = 'public'
          AND table_name NOT LIKE 'audit_log_%'
          AND table_name NOT LIKE 'sync_actions_%'
        ORDER BY table_name
    LOOP
        fk_name := t || '_workspace_id_fkey';
        EXECUTE format('ALTER TABLE %I DROP CONSTRAINT IF EXISTS %I', t, fk_name);
        EXECUTE format('ALTER TABLE %I ALTER COLUMN workspace_id SET DEFAULT 1', t);
        EXECUTE format('ALTER TABLE %I ALTER COLUMN workspace_id DROP NOT NULL', t);
    END LOOP;
END
$$;
