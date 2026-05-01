-- Feature flags for staged rollouts. Two layers:
--
--  * site_settings.feature_flags is the workspace-level default for
--    every flag the app knows about. JSONB-shaped, structure
--    `{ "<flag_name>": <boolean | string | object>, ... }`. Empty
--    object means "all flags at code-default."
--
--  * users.feature_flag_overrides is a per-user JSONB merged on top
--    of the workspace defaults. Used to opt individuals into a
--    feature before flipping the workspace default. Same shape;
--    keys present here win.
--
-- Code resolves a flag by reading workspace, then merging user
-- overrides. Code defaults take effect only when neither layer
-- contains the flag.
--
-- The JSONB value is intentionally untyped at the DB level — the
-- application is the source of truth for flag schema. We could
-- add a flag-registry table later if discoverability of available
-- flags becomes a problem, but for v1 the architecture document
-- and a tiny admin UI suffice.

ALTER TABLE site_settings
    ADD COLUMN feature_flags JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE users
    ADD COLUMN feature_flag_overrides JSONB NOT NULL DEFAULT '{}'::jsonb;
