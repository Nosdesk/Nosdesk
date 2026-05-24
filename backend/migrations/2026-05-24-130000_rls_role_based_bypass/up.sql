-- Phase 3h.4 — move cross-workspace bypass from a GUC-flag to a
-- separate BYPASSRLS role.
--
-- The original design used `app.bypass_workspace_check = 'true'`
-- as a disjunct in every policy. Convention says cross-tenant ops
-- go through `with_actor_bypass_context` which sets the flag; the
-- DB doesn't enforce the convention. Per the postgresql-hackers
-- thread on placeholder-GUC permissions, `app.*` settings are
-- write-by-anyone — `nosdesk_app` itself can do
-- `SELECT set_config('app.bypass_workspace_check', 'true', true)`
-- mid-query and silently neutralise every workspace-isolation
-- policy. Bypass was convention-enforced, not DB-enforced.
--
-- Fix: a separate `nosdesk_admin` role with the `BYPASSRLS`
-- attribute. The runtime `with_actor_bypass_context` does
-- `SET LOCAL ROLE nosdesk_admin` for the transaction; cross-
-- workspace ops gain bypass via role, gated by membership
-- (GRANT nosdesk_admin TO nosdesk_app). A typo or a malicious
-- INSERT can no longer flip the bypass flag because the policies
-- no longer read it.
--
-- This migration also regenerates every existing
-- `_workspace_isolation` policy without the bypass disjunct.

-- 1. Provision `nosdesk_admin`. NOLOGIN because the runtime app
--    role still connects as `nosdesk_app` and elevates via
--    SET ROLE. BYPASSRLS is the attribute that lets queries skip
--    every policy on every table the role can otherwise access.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'nosdesk_admin') THEN
        CREATE ROLE nosdesk_admin NOLOGIN BYPASSRLS;
    END IF;
END
$$;

-- 2. Grant `nosdesk_admin` to `nosdesk_app` so the app role can
--    SET ROLE into it. Without membership, only superusers can
--    SET ROLE freely. This is the gate.
GRANT nosdesk_admin TO nosdesk_app;

-- 3. Mirror `nosdesk_app`'s blanket grants onto `nosdesk_admin`
--    so the elevated role can actually run the queries it's
--    elevated for. Default privileges so future tables (e.g.,
--    Phase 3i migrations) are covered without revisits.
GRANT USAGE ON SCHEMA public TO nosdesk_admin;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO nosdesk_admin;
GRANT USAGE, SELECT, UPDATE ON ALL SEQUENCES IN SCHEMA public TO nosdesk_admin;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO nosdesk_admin;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO nosdesk_admin;

-- 4. Regenerate every `_workspace_isolation` policy without the
--    bypass disjunct. Catches: every tenant table from the POC
--    plus all wave-1 and wave-2 migrations plus every partition
--    child created by sync/partitions.rs and the 3h.1 backfill.
--    Iterating pg_policies makes this self-balancing: if a new
--    tenant table was added in a parallel branch, this still
--    catches it (any policy named `*_workspace_isolation` gets
--    the new shape).
DO $$
DECLARE
    pol record;
    new_predicate constant text :=
        'workspace_id = NULLIF(current_setting(''app.workspace_id'', true), '''')::int';
BEGIN
    FOR pol IN
        SELECT schemaname, tablename, policyname
        FROM pg_policies
        WHERE schemaname = 'public' AND policyname LIKE '%_workspace_isolation'
    LOOP
        EXECUTE format('DROP POLICY %I ON %I.%I',
            pol.policyname, pol.schemaname, pol.tablename);
        EXECUTE format(
            'CREATE POLICY %I ON %I.%I USING (%s) WITH CHECK (%s)',
            pol.policyname, pol.schemaname, pol.tablename,
            new_predicate, new_predicate
        );
    END LOOP;
END
$$;
