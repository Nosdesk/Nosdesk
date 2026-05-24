-- Phase 3h.6 — tighten the blanket GRANTs on `nosdesk_app`.
--
-- The POC migration handed `nosdesk_app` blanket
-- `SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public`
-- so the role could run every tenant query. RLS catches per-row,
-- but if RLS ever gets accidentally disabled (a one-off ALTER
-- TABLE during incident response, a malformed migration), the
-- blanket grant means full table access immediately. The external
-- review recommended REVOKEing on tables that are platform-only
-- or that shouldn't be reachable from app traffic at all.
--
-- Tables this REVOKEs from nosdesk_app:
--
-- 1. `workspaces` and `workspace_members` — workspace identity
--    and membership records. Mutating these is the workspace
--    lifecycle handler's job, and that handler runs through
--    `PlatformConn` (nosdesk_admin, BYPASSRLS). Ordinary
--    handlers must not be able to create / archive / re-key
--    workspaces.
--
-- 2. Partition CHILDREN of `audit_log` and `sync_actions`
--    (e.g. `audit_log_2026_07`, `sync_actions_default`).
--    Postgres' ACL check for partitioned-table queries fires on
--    the PARENT, so REVOKEing on children does NOT break the
--    normal access path. A direct query against a child by name
--    now fails with `permission denied` instead of returning
--    zero rows via RLS — much louder failure mode for any
--    future code that accidentally hardcodes a partition name.
--
-- 3. `__diesel_schema_migrations` — migration metadata. App
--    traffic has no reason to read or write it; migrations are
--    run by the deploy pipeline as the superuser role.
--
-- Tables NOT touched here (callouts so the choice is explicit):
--
-- - User-scoped non-tenant tables (`active_sessions`,
--   `refresh_tokens`, `reset_tokens`, `passkey_credentials`,
--   `user_auth_identities`, `user_emails`, `user_preferences`,
--   `users`): nosdesk_app needs these for auth/login flows.
--   Their access model is "every user can see their own row";
--   that's a separate ownership-check concern, not an RLS
--   workspace-isolation concern.
--
-- - Platform-internal tables that handlers legitimately read
--   (`system_meta`, `site_settings`, etc.): these are not RLS-
--   enabled or are RLS-enabled with workspace_id. Either way
--   nosdesk_app needs access; PlatformConn handles the writes
--   that legitimately cross tenants.
--
-- The same REVOKEs apply to `nosdesk_admin` so the BYPASSRLS role
-- also can't directly touch these — workspace lifecycle should
-- still go through code-reviewed handlers, not ad-hoc SQL.

-- 1. workspaces + workspace_members
--
-- Split the access model: nosdesk_app gets SELECT-only on
-- workspaces (every request needs WorkspaceContextMiddleware to
-- resolve subdomain -> workspace), and nothing on workspace_members
-- (membership management is admin-only). nosdesk_admin keeps full
-- access because workspace lifecycle handlers (create / archive /
-- hard-delete / membership management) run via PlatformConn under
-- the nosdesk_admin role.
REVOKE SELECT, INSERT, UPDATE, DELETE ON workspaces FROM nosdesk_app;
REVOKE SELECT, INSERT, UPDATE, DELETE ON workspace_members FROM nosdesk_app;
-- nosdesk_app keeps SELECT on workspaces for the middleware
-- resolver. Column-level grants would be tighter (only id, slug,
-- name, archived_at, organisation_id are read in middleware) but
-- column ACLs are operationally fiddly; revisit if the threat
-- model warrants more.
GRANT SELECT ON workspaces TO nosdesk_app;

-- 2. Partition children — REVOKE on monthly ranged children only
--    (audit_log_2026_07, sync_actions_2026_07, etc.). The _default
--    parachute partitions are intentionally exempt: the partition
--    rotator's drift check (check_default_partition_drift) queries
--    `SELECT COUNT(*) FROM audit_log_default` to surface the
--    "rotator lagged, rows landed in the parachute" condition.
--    Without nosdesk_app SELECT on the default, that diagnostic
--    becomes a permission-denied error that aborts the rotator's
--    enclosing transaction. The default partition isn't a security
--    boundary anyway — it only collects rows that *should never*
--    land outside a monthly range, and those rows are operator
--    follow-up regardless.
DO $$
DECLARE
    parent_name text;
    child_name text;
BEGIN
    FOR parent_name IN
        SELECT unnest(ARRAY['audit_log', 'sync_actions'])
    LOOP
        FOR child_name IN
            SELECT c.relname
            FROM pg_inherits i
            JOIN pg_class c ON i.inhrelid = c.oid
            WHERE i.inhparent = parent_name::regclass
              AND c.relname NOT LIKE '%_default'
        LOOP
            EXECUTE format(
                'REVOKE SELECT, INSERT, UPDATE, DELETE ON %I FROM nosdesk_app, nosdesk_admin',
                child_name
            );
        END LOOP;
    END LOOP;
END
$$;

-- 3. Diesel migration metadata
REVOKE SELECT, INSERT, UPDATE, DELETE ON __diesel_schema_migrations
    FROM nosdesk_app, nosdesk_admin;

-- 4. Cut auto-inheritance of nosdesk_admin's permissions by
--    nosdesk_app.
--
-- Phase 3h.4 did `GRANT nosdesk_admin TO nosdesk_app` so the app
-- role could `SET ROLE nosdesk_admin` for bypass paths. Default
-- role-grant semantics in Postgres give the member role automatic
-- *inherited* access to everything the parent role can touch —
-- meaning nosdesk_app silently got INSERT / UPDATE / DELETE on
-- workspaces via membership-inheritance, completely defeating
-- the REVOKEs above. (Role *attributes* like BYPASSRLS don't
-- inherit; only *permissions on database objects* do. So the
-- inheritance was worst-of-both: full ACL access without the
-- bypass that's the only reason for the membership.)
--
-- Two-part fix:
--
-- (a) ALTER ROLE nosdesk_app NOINHERIT — defense in depth.
--     Sets the default for any future grants the role receives
--     to INHERIT FALSE.
--
-- (b) Re-grant nosdesk_admin TO nosdesk_app WITH INHERIT FALSE,
--     SET TRUE — Postgres 16+ stores the inherit option on the
--     grant itself (pg_auth_members.inherit_option), not just
--     on the role attribute. The 3h.4 grant was created with
--     the default INHERIT TRUE; we explicitly override here.
--     SET TRUE keeps `SET ROLE nosdesk_admin` working, which is
--     what `with_actor_bypass_context` does.
ALTER ROLE nosdesk_app NOINHERIT;
REVOKE nosdesk_admin FROM nosdesk_app;
GRANT nosdesk_admin TO nosdesk_app WITH INHERIT FALSE, SET TRUE;
