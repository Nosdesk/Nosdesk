-- Extend the audit_log_trigger coverage to the asset domain.
--
-- Same selection rationale as 2026-05-11-210000_attach_audit_tier1:
-- "operator-facing surfaces a sysadmin actually wants a forensic
-- trail for". After Phase A renamed devices to assets and added
-- the runtime kind registry, the two tables hold the canonical
-- inventory + the schema that constrains it. Both deserve a
-- write trail.
--
-- - assets:      who edited this row, when, what changed. Lifecycle
--                surfaces (sync, ticket attach, group membership)
--                already emit their own events; this captures the
--                column-level diffs Intune sync writes and the
--                hand-edits an admin makes in DeviceView.
-- - asset_kinds: even more important than the row table, since a
--                kind edit silently changes the validation schema
--                for every asset that references it. Forensic
--                trail is "this kind's pattern was tightened on
--                2026-05-20, that's why these creates failed".

CREATE TRIGGER tr_audit_assets
    AFTER INSERT OR UPDATE OR DELETE ON assets
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');

CREATE TRIGGER tr_audit_asset_kinds
    AFTER INSERT OR UPDATE OR DELETE ON asset_kinds
    FOR EACH ROW EXECUTE FUNCTION audit_log_trigger('id');
