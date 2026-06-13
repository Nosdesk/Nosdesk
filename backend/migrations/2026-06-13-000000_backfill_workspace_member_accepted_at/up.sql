-- Backfill workspace_members.accepted_at for members who predate the
-- code that stamps it (membership insert + invitation accept). The
-- members list renders a NULL accepted_at as a pending invite, so
-- bootstrap admins, admin-added members, and OAuth-provisioned users
-- all showed "pending" forever.
--
-- Only members who have completed account setup (they hold an auth
-- identity) are backfilled. A genuinely-pending email invitee has no
-- auth identity until they accept, so it is correctly left NULL.
--
-- accepted_at is display-only; the membership 403 gate checks row
-- existence, not this column. The audit trigger sources workspace_id
-- from each row, so this bulk update needs no app.workspace_id GUC.
UPDATE workspace_members wm
SET accepted_at = wm.invited_at
WHERE wm.accepted_at IS NULL
  AND EXISTS (
    SELECT 1 FROM user_auth_identities uai
    WHERE uai.user_uuid = wm.user_uuid
  );
