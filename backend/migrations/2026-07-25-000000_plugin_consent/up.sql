-- Consent gate for the plugin sandbox (4b-3c-3).
--
-- A plugin can now land in `awaiting_consent`: the row exists but is NOT served
-- (the runtime loader serves only `installed`, so this state is inert by
-- construction). An admin approves the requested permission scope to advance it
-- to `installed`. Trusted tiers (official / local) auto-advance at install; the
-- untrusted tiers (verified / community) wait here for consent.
--
-- Additive only (new state value + nullable columns, no backfill), so no audit
-- trigger fires on existing rows.

ALTER TABLE plugins DROP CONSTRAINT plugins_state_check;
ALTER TABLE plugins ADD CONSTRAINT plugins_state_check
  CHECK ((state)::text = ANY (ARRAY[
    'installed'::text, 'disabled'::text, 'quarantined'::text,
    'uninstalled'::text, 'awaiting_consent'::text
  ]));

-- The exact permission set the admin consented to (a JSON array of permission
-- strings). Re-consent is required when a later version's permissions are not a
-- subset of this. NULL until first consent (and for tiers that never consent via
-- the UI, it records the auto-consented set at install).
ALTER TABLE plugins ADD COLUMN consented_permissions jsonb;
ALTER TABLE plugins ADD COLUMN consented_at timestamptz;
ALTER TABLE plugins ADD COLUMN consented_by uuid REFERENCES users(uuid) ON DELETE SET NULL;
