-- Workspace-wide default email signature. When an agent has not set a
-- personal signature in user_preferences.signature, the outbound
-- channel reply pipeline falls through to this column before giving
-- up. NULL = no org default (preserves the pre-migration behaviour;
-- existing deployments that already rely on per-user signatures only
-- see no change).
--
-- TEXT (not VARCHAR(N)) because signature length is a soft admin
-- choice, not a constraint we want to police at the schema layer;
-- the handler caps reasonable size at the application boundary.
ALTER TABLE site_settings
    ADD COLUMN signature_default TEXT NULL;

COMMENT ON COLUMN site_settings.signature_default IS
    'Workspace-level default email signature, used when an agent has not set a personal signature. NULL = no org default.';
