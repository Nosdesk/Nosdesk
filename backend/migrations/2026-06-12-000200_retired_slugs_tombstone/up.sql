-- Slug never-reuse tombstone (P1.2).
--
-- A hard-deleted workspace's slug must never be reusable: the control
-- plane assumes a slug maps to at most one workspace identity for all
-- time (link integrity, cache keying, audit correlation). But hard
-- delete is a cascading DELETE, and removing the workspaces row frees
-- the slug that the UNIQUE(slug) constraint was holding. This table
-- records the slug just before the cascade runs, so create_workspace
-- can keep rejecting it forever without retaining the tenant's data.
--
-- It is a global meta-table (no workspace_id, no RLS), like workspaces
-- itself, and only nosdesk_admin writes it (the create check and the
-- hard-delete both run under the BYPASSRLS role).
CREATE TABLE retired_slugs (
    slug VARCHAR(64) PRIMARY KEY,
    workspace_uuid UUID NOT NULL,
    retired_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE retired_slugs OWNER TO nosdesk_admin;
GRANT SELECT ON retired_slugs TO nosdesk_app;
