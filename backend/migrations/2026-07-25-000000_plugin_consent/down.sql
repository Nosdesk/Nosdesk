ALTER TABLE plugins DROP COLUMN consented_by;
ALTER TABLE plugins DROP COLUMN consented_at;
ALTER TABLE plugins DROP COLUMN consented_permissions;
ALTER TABLE plugins DROP CONSTRAINT plugins_state_check;
ALTER TABLE plugins ADD CONSTRAINT plugins_state_check
  CHECK ((state)::text = ANY (ARRAY[
    'installed'::text, 'disabled'::text, 'quarantined'::text, 'uninstalled'::text
  ]));
