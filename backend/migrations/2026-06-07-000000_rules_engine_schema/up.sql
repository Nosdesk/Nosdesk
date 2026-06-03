-- Phase 1 of the unified rules engine.
--
-- Adds four tables (rules, rule_versions, rule_applications,
-- ticket_rule_runs) plus three enum types that back them. Phase 1
-- only ships the manual trigger surface (the "Actions" toolbar
-- button) so the engine event subscription path is unused for now,
-- but the schema is the same one Phase 2 (event triggers) and
-- Phase 3 (time-elapsed) extend without changes. See
-- docs/rules-and-actions-plan.md sections 4.1 - 4.4.
--
-- The existing assignment_rules / assignment_rule_state /
-- assignment_log tables are untouched here; Phase 2 absorbs them in
-- a hard cutover (decision 23 in the plan).

CREATE TYPE rule_state AS ENUM ('draft', 'dry_run', 'live', 'archived');

CREATE TYPE rule_trigger_kind AS ENUM (
    'manual',
    'ticket_created',
    'ticket_updated',
    'ticket_replied',
    'time_elapsed'
);

CREATE TYPE rule_application_status AS ENUM (
    'succeeded',
    'dry_run',
    'skipped_preflight',
    'skipped_condition_unmet',
    'suppressed_recursion_budget',
    'suppressed_loop_guard',
    'failed'
);

-- =====================================================================
-- rules: the unified Rule entity. Phase 1 only inserts trigger_kind =
-- 'manual' rows; later phases use the same table for event-triggered
-- and time-elapsed rules. The rules_manual_no_conditions CHECK is
-- load-bearing per decision 30: agents picking from the toolbar see
-- every live manual rule (category-filtered), never one that fails
-- preflight because the ticket doesn't match a condition.
-- =====================================================================
CREATE TABLE rules (
    id SERIAL PRIMARY KEY,
    workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    trigger_kind rule_trigger_kind NOT NULL,
    -- Trigger-specific config payload. Shape varies by trigger_kind;
    -- the typed validator in repository::rules enforces it at save.
    trigger_config JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- Condition tree (recursive AND/OR/NOT/leaf). Manual rules MUST
    -- be []. The repository's reads_set derivation walks this tree
    -- to populate the column below.
    conditions JSONB NOT NULL DEFAULT '[]'::jsonb,
    -- Ordered list of typed action objects. Always non-empty.
    actions JSONB NOT NULL,
    -- Derived at save time from conditions / actions. Drives the
    -- self-referential save linter (writes ∩ reads != ∅) and the
    -- engine's skip-on-no-reads-changed optimisation in Phase 2.
    reads_set TEXT[] NOT NULL DEFAULT '{}',
    writes_set TEXT[] NOT NULL DEFAULT '{}',
    state rule_state NOT NULL DEFAULT 'draft',
    -- Lower priority value evaluates earlier (matches the existing
    -- assignment_rules convention).
    priority INTEGER NOT NULL DEFAULT 100,
    last_fired_at TIMESTAMPTZ NULL,
    fire_count INTEGER NOT NULL DEFAULT 0,
    created_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    archived_at TIMESTAMPTZ NULL,
    CONSTRAINT rules_manual_no_conditions CHECK (
        trigger_kind <> 'manual' OR conditions = '[]'::jsonb
    ),
    CONSTRAINT rules_actions_non_empty CHECK (
        jsonb_typeof(actions) = 'array' AND jsonb_array_length(actions) > 0
    )
);

-- General-purpose state filter for the admin list view.
CREATE INDEX rules_workspace_state_idx
    ON rules (workspace_id, state)
    WHERE archived_at IS NULL;

-- Event-engine scan (Phase 2): live rules of a given trigger kind in
-- priority order.
CREATE INDEX rules_workspace_trigger_idx
    ON rules (workspace_id, trigger_kind, priority)
    WHERE archived_at IS NULL AND state = 'live';

-- Agent picker (Phase 1): live manual rules per workspace.
CREATE INDEX rules_manual_pickable_idx
    ON rules (workspace_id)
    WHERE archived_at IS NULL
      AND state = 'live'
      AND trigger_kind = 'manual';

-- =====================================================================
-- rule_versions: every UPDATE on rules writes a snapshot row here.
-- rule_applications.rule_version FKs into (rule_id, version) so the
-- activity feed can deep-link to the exact rule shape that fired.
-- =====================================================================
CREATE TABLE rule_versions (
    id SERIAL PRIMARY KEY,
    rule_id INTEGER NOT NULL REFERENCES rules(id) ON DELETE CASCADE,
    -- Denormalised from rules.workspace_id so RLS can scope this
    -- table without joining the parent. The version-writing trigger
    -- copies NEW.workspace_id.
    workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    trigger_kind rule_trigger_kind NOT NULL,
    trigger_config JSONB NOT NULL,
    conditions JSONB NOT NULL,
    actions JSONB NOT NULL,
    state rule_state NOT NULL,
    priority INTEGER NOT NULL,
    saved_by UUID REFERENCES users(uuid) ON DELETE SET NULL,
    saved_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (rule_id, version)
);

CREATE INDEX rule_versions_rule_recent_idx
    ON rule_versions (rule_id, saved_at DESC);

-- =====================================================================
-- rule_applications: the unified audit log. One row per fire attempt
-- (manual apply, event match, time match, dry-run shadow); the status
-- enum carries why nothing happened when nothing did. condition_eval /
-- actions_taken / actions_skipped / failure_reason are populated only
-- on dry-run + failed + suppressed_* + skipped_*; succeeded rows stay
-- tight (the hot-path retention case).
-- =====================================================================
CREATE TABLE rule_applications (
    id BIGSERIAL PRIMARY KEY,
    workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    rule_id INTEGER NOT NULL REFERENCES rules(id) ON DELETE CASCADE,
    rule_version INTEGER NOT NULL,
    ticket_id INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    status rule_application_status NOT NULL,
    correlation_id UUID NULL,
    actor_uuid UUID REFERENCES users(uuid) ON DELETE SET NULL,
    actor_kind VARCHAR(16) NOT NULL,
    originating_event_id UUID NULL,
    originating_event_kind VARCHAR(64) NULL,
    condition_evaluation JSONB NULL,
    actions_taken JSONB NULL,
    actions_skipped JSONB NULL,
    failure_reason TEXT NULL,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT rule_applications_actor_kind_valid CHECK (
        actor_kind IN ('user', 'system')
    )
);

CREATE INDEX rule_applications_workspace_recent_idx
    ON rule_applications (workspace_id, applied_at DESC);
CREATE INDEX rule_applications_rule_recent_idx
    ON rule_applications (rule_id, applied_at DESC);
CREATE INDEX rule_applications_ticket_idx
    ON rule_applications (ticket_id, applied_at DESC);
CREATE INDEX rule_applications_failures_idx
    ON rule_applications (workspace_id, applied_at DESC)
    WHERE status IN ('failed', 'suppressed_recursion_budget', 'suppressed_loop_guard');

-- =====================================================================
-- ticket_rule_runs: per-event recursion budget. PRIMARY KEY enforces
-- "at most one fire of rule R on ticket T per originating event E";
-- the fired_at column drives the hourly sweeper. Unused in Phase 1
-- (no event subscriber yet) but added now so Phase 2 has the
-- substrate already in place.
-- =====================================================================
CREATE TABLE ticket_rule_runs (
    event_id UUID NOT NULL,
    ticket_id INTEGER NOT NULL,
    rule_id INTEGER NOT NULL,
    fired_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (event_id, ticket_id, rule_id)
);

CREATE INDEX ticket_rule_runs_age_idx
    ON ticket_rule_runs (fired_at);

-- =====================================================================
-- Versioning triggers: every INSERT + meaningful UPDATE on rules
-- writes a rule_versions snapshot row. Numbering is per-rule,
-- monotonically increasing, atomic under the row lock the
-- INSERT/UPDATE takes. We split AFTER INSERT (initial version row,
-- NEW already exists) from BEFORE UPDATE (new version row plus
-- bumping updated_at) because AFTER triggers cannot mutate NEW.
--
-- The UPDATE trigger's WHEN clause compares ONLY the snapshot-
-- relevant content columns. Bookkeeping bumps (fire_count,
-- last_fired_at) from apply_manual must not produce a version row;
-- those columns change on every fire and would otherwise flood
-- rule_versions with identical-content snapshots and misnumber
-- rule_applications.rule_version. The fields below are exactly
-- what rule_versions records.
--
-- saved_by reads from the same `app.actor_uuid` session GUC the
-- audit trigger uses, so an edit by admin B is credited to B even
-- though rules.created_by stays pointed at A (the original
-- creator). The GUC is NULL when the change comes from a system
-- path that didn't go through with_actor_context; we fall back to
-- NEW.created_by in that case rather than recording NULL, since
-- "the original author probably authored this" is a safer default
-- than "we don't know".
-- =====================================================================
CREATE OR REPLACE FUNCTION rules_write_initial_version() RETURNS TRIGGER AS $$
DECLARE
    actor UUID := NULLIF(current_setting('app.actor_uuid', true), '')::UUID;
BEGIN
    INSERT INTO rule_versions (
        rule_id, workspace_id, version, name, description, trigger_kind,
        trigger_config, conditions, actions, state, priority,
        saved_by, saved_at
    )
    VALUES (
        NEW.id, NEW.workspace_id, 1, NEW.name, NEW.description, NEW.trigger_kind,
        NEW.trigger_config, NEW.conditions, NEW.actions, NEW.state, NEW.priority,
        COALESCE(actor, NEW.created_by), NEW.created_at
    );
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE FUNCTION rules_write_update_version() RETURNS TRIGGER AS $$
DECLARE
    next_version INTEGER;
    actor        UUID := NULLIF(current_setting('app.actor_uuid', true), '')::UUID;
BEGIN
    SELECT COALESCE(MAX(version), 0) + 1
    INTO next_version
    FROM rule_versions
    WHERE rule_id = NEW.id;

    INSERT INTO rule_versions (
        rule_id, workspace_id, version, name, description, trigger_kind,
        trigger_config, conditions, actions, state, priority,
        saved_by, saved_at
    )
    VALUES (
        NEW.id, NEW.workspace_id, next_version, NEW.name, NEW.description, NEW.trigger_kind,
        NEW.trigger_config, NEW.conditions, NEW.actions, NEW.state, NEW.priority,
        COALESCE(actor, NEW.created_by), NOW()
    );

    NEW.updated_at := NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER rules_version_on_insert
    AFTER INSERT ON rules
    FOR EACH ROW
    EXECUTE FUNCTION rules_write_initial_version();

CREATE TRIGGER rules_version_on_update
    BEFORE UPDATE ON rules
    FOR EACH ROW
    WHEN (
        (OLD.name, OLD.description, OLD.trigger_kind, OLD.trigger_config,
         OLD.conditions, OLD.actions, OLD.state, OLD.priority,
         OLD.archived_at, OLD.reads_set, OLD.writes_set)
        IS DISTINCT FROM
        (NEW.name, NEW.description, NEW.trigger_kind, NEW.trigger_config,
         NEW.conditions, NEW.actions, NEW.state, NEW.priority,
         NEW.archived_at, NEW.reads_set, NEW.writes_set)
    )
    EXECUTE FUNCTION rules_write_update_version();

-- =====================================================================
-- Row-level security on the three workspace-scoped tables. Same
-- `app.workspace_id` GUC pattern as the existing tenant tables (see
-- `2026-05-24-110007_rls_groups_users` for the canonical shape).
-- `ticket_rule_runs` is engine-internal (sweeper + recursion budget,
-- never read by user-facing handlers) so it stays without RLS; its
-- access surface is gated by callers fetching rules and tickets that
-- are already workspace-scoped.
-- =====================================================================
ALTER TABLE rules ENABLE ROW LEVEL SECURITY;
ALTER TABLE rules FORCE ROW LEVEL SECURITY;
CREATE POLICY rules_workspace_isolation ON rules
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE rule_versions ENABLE ROW LEVEL SECURITY;
ALTER TABLE rule_versions FORCE ROW LEVEL SECURITY;
CREATE POLICY rule_versions_workspace_isolation ON rule_versions
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

ALTER TABLE rule_applications ENABLE ROW LEVEL SECURITY;
ALTER TABLE rule_applications FORCE ROW LEVEL SECURITY;
CREATE POLICY rule_applications_workspace_isolation ON rule_applications
    USING (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    )
    WITH CHECK (
        workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int
        OR NULLIF(current_setting('app.bypass_workspace_check', true), '') = 'true'
    );

-- =====================================================================
-- Transfer ownership to nosdesk_admin so future DDL run by a less-
-- privileged operator still owns these. Mirrors the M5 task-9 pattern
-- from `2026-06-03-040000_tenant_table_ownership` but applied at
-- creation time rather than retroactively. Sequences come along
-- because they're SERIAL-derived.
-- =====================================================================
ALTER TABLE rules OWNER TO nosdesk_admin;
ALTER TABLE rule_versions OWNER TO nosdesk_admin;
ALTER TABLE rule_applications OWNER TO nosdesk_admin;
ALTER TABLE ticket_rule_runs OWNER TO nosdesk_admin;

ALTER SEQUENCE rules_id_seq OWNER TO nosdesk_admin;
ALTER SEQUENCE rule_versions_id_seq OWNER TO nosdesk_admin;
ALTER SEQUENCE rule_applications_id_seq OWNER TO nosdesk_admin;

ALTER FUNCTION rules_write_initial_version() OWNER TO nosdesk_admin;
ALTER FUNCTION rules_write_update_version() OWNER TO nosdesk_admin;

-- =====================================================================
-- Grants for the application role. `nosdesk_app` runs as a non-
-- superuser and is subject to FORCE RLS; the GRANT lets it read /
-- write rows that pass the workspace_id policy. Pattern matches
-- the existing tenant tables.
-- =====================================================================
GRANT SELECT, INSERT, UPDATE, DELETE ON rules TO nosdesk_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON rule_versions TO nosdesk_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON rule_applications TO nosdesk_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON ticket_rule_runs TO nosdesk_app;

GRANT USAGE ON SEQUENCE rules_id_seq TO nosdesk_app;
GRANT USAGE ON SEQUENCE rule_versions_id_seq TO nosdesk_app;
GRANT USAGE ON SEQUENCE rule_applications_id_seq TO nosdesk_app;
