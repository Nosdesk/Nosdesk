-- Phase 3d — make workspace_id NOT NULL and FK to workspaces(id)
-- on every tenant table.
--
-- Now safe because Phase 3a-3c.2 RLS policies + Phase 3h.4
-- BYPASSRLS role split + Phase 3h.2 actor_for workspace pin all
-- ensure that every INSERT either carries a workspace pin OR
-- goes through PlatformConn with an explicit workspace value.
-- The defensive default value (Phase 1's `DEFAULT 1`) is
-- replaced with a GUC-reading default so Diesel inserts that
-- don't list workspace_id continue to work — they just get
-- the request's workspace from the GUC instead of a static `1`.
--
-- Three operations per table, all online-safe:
--
-- 1. ALTER COLUMN ... SET NOT NULL — cheap in Postgres 12+
--    because every row already has a non-NULL value (Phase 1's
--    DEFAULT 1 backfill + zero NULLs verified pre-migration).
--    Postgres records the column's NOT NULL state without
--    rewriting the table.
--
-- 2. Replace `DEFAULT 1` with
--    `DEFAULT NULLIF(current_setting('app.workspace_id', true), '')::int`
--    so future inserts that don't explicitly name workspace_id
--    inherit the request's workspace from the GUC. With NOT
--    NULL active and the GUC absent, the insert fails loudly
--    (NULL → NOT NULL violation) rather than silently writing
--    workspace_id=1.
--
-- 3. ADD CONSTRAINT FOREIGN KEY (workspace_id) REFERENCES
--    workspaces(id) NOT VALID, then VALIDATE CONSTRAINT. The
--    NOT VALID + VALIDATE split avoids the long lock: ADD NOT
--    VALID takes ACCESS EXCLUSIVE briefly but doesn't scan
--    existing rows; VALIDATE takes only SHARE UPDATE EXCLUSIVE
--    so concurrent reads and writes continue.
--
-- Partition children (audit_log_*, sync_actions_*) inherit the
-- column changes from the parent automatically (Postgres
-- partition semantics); the loop below skips them.

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

        EXECUTE format('ALTER TABLE %I ALTER COLUMN workspace_id SET NOT NULL', t);

        EXECUTE format(
            'ALTER TABLE %I ALTER COLUMN workspace_id SET DEFAULT '
            'NULLIF(current_setting(''app.workspace_id'', true), '''')::int',
            t
        );

        -- Drop any prior FK with the same name (idempotent on re-run).
        EXECUTE format('ALTER TABLE %I DROP CONSTRAINT IF EXISTS %I', t, fk_name);
        EXECUTE format(
            'ALTER TABLE %I ADD CONSTRAINT %I '
            'FOREIGN KEY (workspace_id) REFERENCES workspaces(id) NOT VALID',
            t, fk_name
        );
        EXECUTE format('ALTER TABLE %I VALIDATE CONSTRAINT %I', t, fk_name);
    END LOOP;
END
$$;
