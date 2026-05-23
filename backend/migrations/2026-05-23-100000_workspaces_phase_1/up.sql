-- Multi-tenant migration, Phase 1.
--
-- Lays the foundation for the pool-model multi-tenancy locked
-- in `docs/multi-tenant-migration-plan.md` (D1-D6, 2026-05-08),
-- with audit refinements layered on (workspace id pinning, role
-- + slug CHECKs, idempotent ALTERs, default-1 inserts).
--
-- Five things happen, in order:
--   1. Drop an orphaned `set_updated_at` trigger that blocks
--      any UPDATE on `documentation_revisions` (column was
--      removed previously but the trigger registration wasn't).
--      The backfill UPDATEs below need this gone.
--   2. Create the `workspaces` table with a slug-format CHECK
--      and bootstrap the default workspace at *explicit* id=1,
--      then bump the sequence so the next workspace gets id=2.
--      Pinning the id removes the magic-number `workspace_id=1`
--      backfill below from relying on SERIAL ordering.
--   3. Create `workspace_members` with a role CHECK
--      (`owner|admin|member`) and FK back to users. All existing
--      non-deleted users become members of the default workspace,
--      preserving admin status.
--   4. Add `workspace_id INTEGER DEFAULT 1` (nullable) to every
--      tenant table. `ADD COLUMN ... DEFAULT 1` is metadata-only
--      on Postgres >= 11 (stored in `attmissingval`), so no
--      cost difference vs the bare form, AND any old-app INSERTs
--      during the Phase 1 -> Phase 4 transition get the right
--      value instead of NULL. The default gets dropped in Phase
--      4 alongside the NOT NULL/FK promotion.
--      `ADD COLUMN IF NOT EXISTS` for idempotency on partially
--      applied dev DBs.
--   5. Phase 2 wires the WorkspaceContext middleware + scopes
--      every repo function; Phase 3 adds handler tests; Phase 4
--      enforces NOT NULL + FK + indexes + RLS.
--
-- Per-user satellite tables (auth identities, sessions, tokens,
-- preferences, passkeys, security events, emails) stay global
-- because users are global in the locked design (D4) — a single
-- user can be a member of multiple workspaces, and the
-- satellites travel with the user.
--
-- Plugin registry tables (trusted publishers, local signing
-- key, registry state) stay global: deployment-wide state, not
-- per-tenant.
--
-- `csp_reports` carries workspace_id but the column should
-- remain nullable forever: CSP violations can arrive from any
-- page (login, marketing) where workspace context isn't known.
-- Phase 4's NOT NULL sweep must skip it.

-- =====================================================
-- 1. Drop orphaned trigger
-- =====================================================
DROP TRIGGER IF EXISTS set_updated_at ON documentation_revisions;

-- =====================================================
-- 2. workspaces table + bootstrap
-- =====================================================

CREATE TABLE workspaces (
    id          SERIAL PRIMARY KEY,
    uuid        UUID NOT NULL UNIQUE DEFAULT gen_random_uuid(),
    -- Slug is the subdomain segment (acme.nosdesk.com -> 'acme').
    -- DNS-safe label, lowercase alphanumeric + dash, 1-64 chars,
    -- can't start or end with dash. The 1-char form is reserved
    -- for the `default` self-hosted bootstrap below.
    slug        VARCHAR(64) NOT NULL UNIQUE
                CHECK (slug ~ '^[a-z0-9]([a-z0-9-]{0,62}[a-z0-9])?$'),
    name        VARCHAR(200) NOT NULL,
    -- Opaque plan identifier (free / starter / pro / enterprise
    -- / self_hosted). Intentionally not CHECK-constrained: the
    -- billing surface will churn these values, and a tight CHECK
    -- here would force a migration on every plan rename.
    plan        VARCHAR(32) NOT NULL DEFAULT 'free',
    -- Per-workspace settings JSON. Will absorb site_settings'
    -- per-tenant config in a later phase; until then
    -- site_settings carries its own workspace_id and stays the
    -- single-row bootstrap holder for the default workspace.
    settings    JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    archived_at TIMESTAMPTZ
);

-- Bootstrap workspace at explicit id=1. Pinning the id lets the
-- per-table `UPDATE ... SET workspace_id = 1` below name the
-- workspace by id rather than rely on SERIAL ordering. The
-- setval call after the INSERT bumps the sequence so the next
-- workspace (Phase 5 hosted signup) gets id=2.
INSERT INTO workspaces (id, uuid, slug, name, plan)
VALUES (1, gen_random_uuid(), 'default', 'Workspace', 'self_hosted');

SELECT setval(pg_get_serial_sequence('workspaces', 'id'), 1, true);

-- =====================================================
-- 3. workspace_members table + backfill
-- =====================================================

CREATE TABLE workspace_members (
    workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_uuid    UUID NOT NULL REFERENCES users(uuid) ON DELETE CASCADE,
    -- Workspace-scoped role layered on top of the user's global
    -- role (which keeps governing platform-wide flows like
    -- password reset). CHECK locks the vocabulary down at the
    -- schema layer so Phase 2 code doesn't have to defend
    -- against typos.
    role         VARCHAR(32) NOT NULL
                 CHECK (role IN ('owner', 'admin', 'member')),
    invited_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    accepted_at  TIMESTAMPTZ,
    PRIMARY KEY (workspace_id, user_uuid)
);

CREATE INDEX idx_workspace_members_user
    ON workspace_members (user_uuid);

-- Backfill: every existing non-deleted user joins the default
-- workspace, preserving their admin / non-admin status. Soft-
-- deleted users are skipped; the restore path will need to add
-- the membership back if they're ever restored.
INSERT INTO workspace_members (workspace_id, user_uuid, role, accepted_at)
SELECT
    1,
    uuid,
    CASE WHEN role = 'admin' THEN 'admin' ELSE 'member' END,
    NOW()
FROM users
WHERE deleted_at IS NULL;

-- =====================================================
-- 4. Tenant tables: add nullable workspace_id DEFAULT 1
-- =====================================================
-- One ALTER per table. The column is nullable + DEFAULT 1 for
-- Phase 1; Phase 4 will drop the DEFAULT, add NOT NULL + FK +
-- index per table once every query scopes by workspace.
-- IF NOT EXISTS makes the ALTERs idempotent on partially
-- applied dev DBs.
--
-- The DEFAULT 1 carries two consequences:
--   - Existing rows get backfilled to 1 implicitly (Postgres
--     >= 11 stores constant defaults in `attmissingval` with no
--     row rewrite).
--   - Any old-code INSERT mid-deploy during Phase 1 -> Phase 4
--     gets workspace_id = 1 automatically rather than NULL,
--     eliminating a class of rolling-deploy race conditions
--     and obviating a "Phase 3.5 re-backfill" step.

-- Core ticket flow
ALTER TABLE tickets ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE comments ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE attachments ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE linked_tickets ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE ticket_categories ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE ticket_assets ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE ticket_tags ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE ticket_watchers ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE tags ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;

-- Cycles + projects
ALTER TABLE cycles ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE cycle_tickets ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE projects ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE project_tickets ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;

-- Workflow + assignment
ALTER TABLE workflow_states ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE assignment_rules ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE assignment_rule_state ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE assignment_log ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;

-- SLA
ALTER TABLE sla_policies ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE working_calendars ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE working_calendar_holidays ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;

-- Assets / inventory
ALTER TABLE assets ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE asset_groups ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE asset_kinds ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE asset_usage_log ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE asset_audits ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;

-- Groups
ALTER TABLE groups ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE user_groups ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE group_includes ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE category_group_visibility ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;

-- Documentation
ALTER TABLE documentation_pages ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE documentation_collections ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE documentation_collection_pages ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE documentation_collection_visibility ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE documentation_page_visibility ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE documentation_page_embeddings ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE documentation_page_tickets ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE documentation_starred_pages ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE documentation_subscriptions ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE documentation_revisions ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE article_contents ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE article_content_revisions ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE knowledge_gaps ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE knowledge_gap_signals ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;

-- Channels (per-workspace inbound flows)
ALTER TABLE channels ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE channel_credentials ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE channel_messages ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE outbound_emails ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;

-- Auth (workspace-scoped tokens + webhooks)
ALTER TABLE api_tokens ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE webhooks ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE webhook_deliveries ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;

-- Plugins (per-workspace install + data)
ALTER TABLE plugins ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE plugin_data ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE plugin_activity ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE plugin_collection_rows ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE plugin_collection_schemas ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;

-- Sync (partitioned parents propagate the column to all
-- existing + future partitions automatically in Postgres >= 10)
ALTER TABLE sync_actions ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE sync_delta_tokens ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE sync_history ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;

-- Notifications
ALTER TABLE notifications ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE notification_preferences ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;

-- Audit (partitioned parent propagates)
ALTER TABLE audit_log ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;

-- Operational / ops tables. csp_reports gets the column for
-- consistency but Phase 4 must keep it nullable: violations
-- come from any page, including login / marketing, where the
-- workspace context isn't always known.
ALTER TABLE import_jobs ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE backup_jobs ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE csp_reports ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE search_query_log ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;

-- View / list state (saved views, canned responses)
ALTER TABLE saved_views ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE canned_responses ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
ALTER TABLE user_ticket_views ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;

-- Site settings: single-row global today; for Phase 1 it's a
-- per-workspace settings holder, anchored to the default
-- workspace. A later phase will fold this into
-- workspaces.settings JSONB and drop the table.
ALTER TABLE site_settings ADD COLUMN IF NOT EXISTS workspace_id INTEGER DEFAULT 1;
